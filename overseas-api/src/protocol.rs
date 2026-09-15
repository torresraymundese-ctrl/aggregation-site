use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: &'static str,
    pub message: String,
}

impl ProtocolError {
    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: "UNSUPPORTED_PARAMETER",
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_REQUEST",
            message: message.into(),
        }
    }
}

fn data_url(url: &str) -> Result<(&str, &str), ProtocolError> {
    let value = url
        .strip_prefix("data:")
        .ok_or_else(|| ProtocolError::invalid("Expected a data URL"))?;
    let (metadata, data) = value
        .split_once(',')
        .ok_or_else(|| ProtocolError::invalid("Malformed data URL"))?;
    let mime = metadata
        .strip_suffix(";base64")
        .ok_or_else(|| ProtocolError::invalid("Only base64 data URLs are supported"))?;
    if mime.is_empty() || data.is_empty() {
        return Err(ProtocolError::invalid("Malformed data URL"));
    }
    Ok((mime, data))
}

fn content_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    content.as_array().map(|parts| {
        parts
            .iter()
            .filter_map(|part| {
                (part.get("type").and_then(Value::as_str) == Some("text"))
                    .then(|| part.get("text").and_then(Value::as_str))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn system_text(messages: &[Value]) -> String {
    messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .filter_map(|message| message.get("content").and_then(content_text))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn openai_tools(input: &Value) -> Result<Vec<Value>, ProtocolError> {
    let Some(tools) = input.get("tools") else {
        return Ok(Vec::new());
    };
    let tools = tools
        .as_array()
        .ok_or_else(|| ProtocolError::invalid("tools must be an array"))?;
    tools
        .iter()
        .map(|tool| {
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProtocolError::unsupported(
                    "Only function tools are supported",
                ));
            }
            let function = tool
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| ProtocolError::invalid("Function tool is missing function"))?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtocolError::invalid("Function tool is missing name"))?;
            let parameters = function
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            Ok(json!({
                "name": name,
                "description": function.get("description").and_then(Value::as_str).unwrap_or(""),
                "parameters": parameters
            }))
        })
        .collect()
}

fn tool_names_by_call_id(messages: &[Value]) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for message in messages {
        let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
            continue;
        };
        for call in calls {
            if let (Some(id), Some(name)) = (
                call.get("id").and_then(Value::as_str),
                call.pointer("/function/name").and_then(Value::as_str),
            ) {
                result.insert(id.to_string(), name.to_string());
            }
        }
    }
    result
}

fn anthropic_content(message: &Value) -> Result<Vec<Value>, ProtocolError> {
    let mut blocks = Vec::new();
    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            if !text.is_empty() {
                blocks.push(json!({"type": "text", "text": text}));
            }
        } else if let Some(parts) = content.as_array() {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => blocks.push(json!({
                        "type": "text",
                        "text": part.get("text").and_then(Value::as_str).unwrap_or("")
                    })),
                    Some("image_url") => {
                        let url = part
                            .pointer("/image_url/url")
                            .and_then(Value::as_str)
                            .ok_or_else(|| ProtocolError::invalid("image_url.url is required"))?;
                        let source = if url.starts_with("data:") {
                            let (media_type, data) = data_url(url)?;
                            json!({"type": "base64", "media_type": media_type, "data": data})
                        } else if url.starts_with("https://") {
                            json!({"type": "url", "url": url})
                        } else {
                            return Err(ProtocolError::invalid(
                                "Image URL must use HTTPS or a base64 data URL",
                            ));
                        };
                        blocks.push(json!({"type": "image", "source": source}));
                    }
                    Some(kind) => {
                        return Err(ProtocolError::unsupported(format!(
                            "Anthropic does not support OpenAI content part type {kind}"
                        )))
                    }
                    None => return Err(ProtocolError::invalid("Content part type is required")),
                }
            }
        } else if !content.is_null() {
            return Err(ProtocolError::invalid(
                "Message content must be text or an array",
            ));
        }
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtocolError::invalid("Tool call is missing id"))?;
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtocolError::invalid("Tool call is missing function name"))?;
            let arguments = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let parsed: Value = serde_json::from_str(arguments)
                .map_err(|_| ProtocolError::invalid("Tool call arguments must be valid JSON"))?;
            blocks.push(json!({"type": "tool_use", "id": id, "name": name, "input": parsed}));
        }
    }
    Ok(blocks)
}

pub fn openai_to_anthropic(input: &Value, upstream_model: &str) -> Result<Value, ProtocolError> {
    reject_parameters(
        input,
        &[
            "n",
            "seed",
            "presence_penalty",
            "frequency_penalty",
            "logit_bias",
        ],
    )?;
    let messages = input
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ProtocolError::invalid("messages must be an array"))?;
    let mut converted = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtocolError::invalid("Message role is required"))?;
        if role == "system" {
            continue;
        }
        if role == "tool" {
            let tool_use_id = message
                .get("tool_call_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtocolError::invalid("tool_call_id is required"))?;
            let content = message
                .get("content")
                .and_then(content_text)
                .unwrap_or_default();
            converted.push(json!({
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": tool_use_id, "content": content}]
            }));
            continue;
        }
        if role != "user" && role != "assistant" {
            return Err(ProtocolError::unsupported(format!(
                "Anthropic does not support message role {role}"
            )));
        }
        converted.push(json!({"role": role, "content": anthropic_content(message)?}));
    }
    let max_tokens = input
        .get("max_completion_tokens")
        .or_else(|| input.get("max_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(1024);
    let mut body = json!({
        "model": upstream_model,
        "max_tokens": max_tokens,
        "messages": converted
    });
    let system = system_text(messages);
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    copy_if_present(input, &mut body, "temperature", "temperature");
    copy_if_present(input, &mut body, "top_p", "top_p");
    copy_if_present(input, &mut body, "stop", "stop_sequences");

    let tools = openai_tools(input)?;
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .into_iter()
                .map(|mut tool| {
                    let parameters = tool
                        .get_mut("parameters")
                        .map(Value::take)
                        .unwrap_or_default();
                    tool.as_object_mut()
                        .expect("tool is an object")
                        .insert("input_schema".to_string(), parameters);
                    tool
                })
                .collect(),
        );
    }
    if let Some(choice) = input.get("tool_choice") {
        body["tool_choice"] = anthropic_tool_choice(choice)?;
    }
    if let Some(format) = input.get("response_format") {
        let kind = format.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "json_schema" {
            return Err(ProtocolError::unsupported(
                "Anthropic structured output requires response_format.type=json_schema",
            ));
        }
        let schema = format
            .pointer("/json_schema/schema")
            .cloned()
            .ok_or_else(|| {
                ProtocolError::invalid("response_format.json_schema.schema is required")
            })?;
        body["output_config"] = json!({"format": {"type": "json_schema", "schema": schema}});
    }
    Ok(body)
}

fn anthropic_tool_choice(choice: &Value) -> Result<Value, ProtocolError> {
    if let Some(kind) = choice.as_str() {
        return match kind {
            "auto" => Ok(json!({"type": "auto"})),
            "required" => Ok(json!({"type": "any"})),
            "none" => Err(ProtocolError::unsupported(
                "Anthropic tool_choice=none is not supported; omit tools instead",
            )),
            _ => Err(ProtocolError::invalid("Invalid tool_choice")),
        };
    }
    let name = choice
        .pointer("/function/name")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::invalid("tool_choice function name is required"))?;
    Ok(json!({"type": "tool", "name": name}))
}

pub fn anthropic_to_openai(response: &Value, model: &str) -> Value {
    let blocks = response
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text = blocks
        .iter()
        .filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tool_calls: Vec<Value> = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|block| {
            json!({
                "id": block.get("id").and_then(Value::as_str).unwrap_or(""),
                "type": "function",
                "function": {
                    "name": block.get("name").and_then(Value::as_str).unwrap_or(""),
                    "arguments": serde_json::to_string(block.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
                }
            })
        })
        .collect();
    let input_tokens = response
        .pointer("/usage/input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = response
        .pointer("/usage/output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut message = json!({"role": "assistant", "content": text});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let stop_reason = response
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn");
    let finish_reason = match stop_reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        other => other,
    };
    json!({
        "id": response.get("id").and_then(Value::as_str).unwrap_or("chatcmpl-anthropic"),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
        "usage": {"prompt_tokens": input_tokens, "completion_tokens": output_tokens, "total_tokens": input_tokens + output_tokens}
    })
}

fn gemini_parts(
    message: &Value,
    call_names: &HashMap<String, String>,
) -> Result<Vec<Value>, ProtocolError> {
    let mut parts = Vec::new();
    let is_tool_result = message.get("role").and_then(Value::as_str) == Some("tool");
    if !is_tool_result {
        if let Some(content) = message.get("content") {
            if let Some(text) = content.as_str() {
                if !text.is_empty() {
                    parts.push(json!({"text": text}));
                }
            } else if let Some(items) = content.as_array() {
                for item in items {
                    match item.get("type").and_then(Value::as_str) {
                        Some("text") => parts.push(
                            json!({"text": item.get("text").and_then(Value::as_str).unwrap_or("")}),
                        ),
                        Some("image_url") => {
                            let url = item
                                .pointer("/image_url/url")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    ProtocolError::invalid("image_url.url is required")
                                })?;
                            if url.starts_with("data:") {
                                let (mime_type, data) = data_url(url)?;
                                parts.push(
                                    json!({"inlineData": {"mimeType": mime_type, "data": data}}),
                                );
                            } else if url.starts_with("https://") {
                                let mime_type = item
                                    .pointer("/image_url/mime_type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("image/jpeg");
                                parts.push(
                                    json!({"fileData": {"mimeType": mime_type, "fileUri": url}}),
                                );
                            } else {
                                return Err(ProtocolError::invalid(
                                    "Image URL must use HTTPS or a base64 data URL",
                                ));
                            }
                        }
                        Some(kind) => {
                            return Err(ProtocolError::unsupported(format!(
                                "Gemini does not support OpenAI content part type {kind}"
                            )))
                        }
                        None => {
                            return Err(ProtocolError::invalid("Content part type is required"))
                        }
                    }
                }
            } else if !content.is_null() {
                return Err(ProtocolError::invalid(
                    "Message content must be text or an array",
                ));
            }
        }
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtocolError::invalid("Tool call is missing function name"))?;
            let arguments = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let args: Value = serde_json::from_str(arguments)
                .map_err(|_| ProtocolError::invalid("Tool call arguments must be valid JSON"))?;
            let mut function_call = json!({"name": name, "args": args});
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                function_call["id"] = json!(id);
            }
            parts.push(json!({"functionCall": function_call}));
        }
    }
    if is_tool_result {
        let id = message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtocolError::invalid("tool_call_id is required"))?;
        let name = call_names.get(id).ok_or_else(|| {
            ProtocolError::invalid("tool_call_id does not match an earlier tool call")
        })?;
        let text = message
            .get("content")
            .and_then(content_text)
            .unwrap_or_default();
        let response =
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({"result": text}));
        parts.push(json!({"functionResponse": {"id": id, "name": name, "response": response}}));
    }
    Ok(parts)
}

pub fn openai_to_gemini(input: &Value) -> Result<Value, ProtocolError> {
    reject_parameters(
        input,
        &[
            "seed",
            "presence_penalty",
            "frequency_penalty",
            "logit_bias",
        ],
    )?;
    let messages = input
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ProtocolError::invalid("messages must be an array"))?;
    let call_names = tool_names_by_call_id(messages);
    let mut contents = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtocolError::invalid("Message role is required"))?;
        if role == "system" {
            continue;
        }
        let gemini_role = if role == "assistant" { "model" } else { "user" };
        contents.push(json!({"role": gemini_role, "parts": gemini_parts(message, &call_names)?}));
    }
    let mut generation_config = Map::new();
    generation_config.insert(
        "maxOutputTokens".to_string(),
        input
            .get("max_completion_tokens")
            .or_else(|| input.get("max_tokens"))
            .cloned()
            .unwrap_or(json!(1024)),
    );
    for (source, target) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("n", "candidateCount"),
        ("stop", "stopSequences"),
    ] {
        if let Some(value) = input.get(source) {
            generation_config.insert(target.to_string(), value.clone());
        }
    }
    if let Some(format) = input.get("response_format") {
        let kind = format.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "json_schema" => {
                generation_config.insert("responseMimeType".to_string(), json!("application/json"));
                let schema = format
                    .pointer("/json_schema/schema")
                    .cloned()
                    .ok_or_else(|| {
                        ProtocolError::invalid("response_format.json_schema.schema is required")
                    })?;
                generation_config.insert("responseSchema".to_string(), schema);
            }
            "json_object" => {
                generation_config.insert("responseMimeType".to_string(), json!("application/json"));
            }
            _ => {
                return Err(ProtocolError::unsupported(
                    "Unsupported response_format for Gemini",
                ))
            }
        }
    }
    let mut body = json!({"contents": contents, "generationConfig": generation_config});
    let system = system_text(messages);
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts": [{"text": system}]});
    }
    let tools = openai_tools(input)?;
    if !tools.is_empty() {
        body["tools"] = json!([{"functionDeclarations": tools}]);
    }
    if let Some(choice) = input.get("tool_choice") {
        body["toolConfig"] = json!({"functionCallingConfig": gemini_tool_choice(choice)?});
    }
    Ok(body)
}

fn gemini_tool_choice(choice: &Value) -> Result<Value, ProtocolError> {
    if let Some(kind) = choice.as_str() {
        return match kind {
            "auto" => Ok(json!({"mode": "AUTO"})),
            "required" => Ok(json!({"mode": "ANY"})),
            "none" => Ok(json!({"mode": "NONE"})),
            _ => Err(ProtocolError::invalid("Invalid tool_choice")),
        };
    }
    let name = choice
        .pointer("/function/name")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::invalid("tool_choice function name is required"))?;
    Ok(json!({"mode": "ANY", "allowedFunctionNames": [name]}))
}

pub fn gemini_to_openai(response: &Value, model: &str) -> Value {
    let parts = response
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text = parts
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    let tool_calls: Vec<Value> = parts.iter().filter_map(|part| part.get("functionCall")).enumerate().map(|(index, call)| {
        json!({
            "id": call.get("id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("call_gemini_{index}")),
            "type": "function",
            "function": {
                "name": call.get("name").and_then(Value::as_str).unwrap_or(""),
                "arguments": serde_json::to_string(call.get("args").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
            }
        })
    }).collect();
    let mut message = json!({"role": "assistant", "content": text});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let input_tokens = response
        .pointer("/usageMetadata/promptTokenCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = response
        .pointer("/usageMetadata/candidatesTokenCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_tokens = response
        .pointer("/usageMetadata/totalTokenCount")
        .and_then(Value::as_i64)
        .unwrap_or(input_tokens + output_tokens);
    let upstream_finish = response
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str)
        .unwrap_or("STOP");
    let finish_reason = if !message.get("tool_calls").unwrap_or(&Value::Null).is_null() {
        "tool_calls"
    } else {
        match upstream_finish {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            other => other,
        }
    };
    json!({
        "id": format!("chatcmpl-gemini-{}", crate::utils::id_generator::generate_request_id()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
        "usage": {"prompt_tokens": input_tokens, "completion_tokens": output_tokens, "total_tokens": total_tokens}
    })
}

fn reject_parameters(input: &Value, names: &[&str]) -> Result<(), ProtocolError> {
    if let Some(name) = names.iter().find(|name| input.get(**name).is_some()) {
        return Err(ProtocolError::unsupported(format!(
            "Parameter {name} is not supported by the selected provider"
        )));
    }
    Ok(())
}

fn copy_if_present(input: &Value, output: &mut Value, source: &str, target: &str) {
    if let Some(value) = input.get(source) {
        output[target] = value.clone();
    }
}

pub fn responses_to_chat(input: &Value) -> Result<Value, ProtocolError> {
    for unsupported in [
        "previous_response_id",
        "background",
        "include",
        "conversation",
    ] {
        if input.get(unsupported).is_some() {
            return Err(ProtocolError::unsupported(format!(
                "Responses parameter {unsupported} is not supported"
            )));
        }
    }
    if input.get("stream").and_then(Value::as_bool) == Some(true) {
        return Err(ProtocolError::unsupported(
            "Streaming Responses events are not supported; use /v1/chat/completions for SSE",
        ));
    }
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::invalid("model is required"))?;
    let mut messages = Vec::new();
    if let Some(instructions) = input.get("instructions").and_then(Value::as_str) {
        messages.push(json!({"role": "system", "content": instructions}));
    }
    match input.get("input") {
        Some(Value::String(text)) => messages.push(json!({"role": "user", "content": text})),
        Some(Value::Array(items)) => {
            for item in items {
                let kind = item
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("message");
                match kind {
                    "message" => {
                        let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                        let content =
                            responses_content_to_chat(item.get("content").unwrap_or(&Value::Null))?;
                        messages.push(json!({"role": role, "content": content}));
                    }
                    "function_call" => {
                        let id = item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ProtocolError::invalid("function_call call_id is required")
                            })?;
                        let name = item.get("name").and_then(Value::as_str).ok_or_else(|| {
                            ProtocolError::invalid("function_call name is required")
                        })?;
                        let arguments = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        messages.push(json!({
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [{"id": id, "type": "function", "function": {"name": name, "arguments": arguments}}]
                        }));
                    }
                    "function_call_output" => {
                        let id = item.get("call_id").and_then(Value::as_str).ok_or_else(|| {
                            ProtocolError::invalid("function_call_output call_id is required")
                        })?;
                        let output = item.get("output").and_then(Value::as_str).unwrap_or("");
                        messages
                            .push(json!({"role": "tool", "tool_call_id": id, "content": output}));
                    }
                    other => {
                        return Err(ProtocolError::unsupported(format!(
                            "Responses input item type {other} is not supported"
                        )))
                    }
                }
            }
        }
        _ => return Err(ProtocolError::invalid("input must be text or an array")),
    }
    let mut chat = json!({"model": model, "messages": messages, "stream": false});
    copy_if_present(input, &mut chat, "temperature", "temperature");
    copy_if_present(input, &mut chat, "top_p", "top_p");
    copy_if_present(
        input,
        &mut chat,
        "max_output_tokens",
        "max_completion_tokens",
    );
    copy_if_present(input, &mut chat, "tool_choice", "tool_choice");
    if let Some(tools) = input.get("tools").and_then(Value::as_array) {
        let converted: Result<Vec<Value>, ProtocolError> = tools
            .iter()
            .map(|tool| {
                if tool.get("type").and_then(Value::as_str) != Some("function") {
                    return Err(ProtocolError::unsupported(
                        "Only function tools are supported by the Responses adapter",
                    ));
                }
                let name = tool.get("name").and_then(Value::as_str)
                    .ok_or_else(|| ProtocolError::invalid("Function tool name is required"))?;
                Ok(json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": tool.get("description").and_then(Value::as_str).unwrap_or(""),
                        "parameters": tool.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
                        "strict": tool.get("strict").and_then(Value::as_bool).unwrap_or(false)
                    }
                }))
            })
            .collect();
        chat["tools"] = Value::Array(converted?);
    }
    if let Some(format) = input.pointer("/text/format") {
        let kind = format.get("type").and_then(Value::as_str).unwrap_or("");
        chat["response_format"] = match kind {
            "json_schema" => json!({
                "type": "json_schema",
                "json_schema": {
                    "name": format.get("name").and_then(Value::as_str).unwrap_or("response"),
                    "schema": format.get("schema").cloned().ok_or_else(|| ProtocolError::invalid("text.format.schema is required"))?,
                    "strict": format.get("strict").and_then(Value::as_bool).unwrap_or(false)
                }
            }),
            "json_object" => json!({"type": "json_object"}),
            "text" => Value::Null,
            other => {
                return Err(ProtocolError::unsupported(format!(
                    "Responses text format {other} is not supported"
                )))
            }
        };
        if chat["response_format"].is_null() {
            chat.as_object_mut()
                .expect("chat object")
                .remove("response_format");
        }
    }
    Ok(chat)
}

fn responses_content_to_chat(content: &Value) -> Result<Value, ProtocolError> {
    if let Some(text) = content.as_str() {
        return Ok(json!(text));
    }
    let items = content.as_array().ok_or_else(|| {
        ProtocolError::invalid("Responses message content must be text or an array")
    })?;
    let mut result = Vec::new();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("input_text") | Some("output_text") => result.push(json!({
                "type": "text",
                "text": item.get("text").and_then(Value::as_str).unwrap_or("")
            })),
            Some("input_image") => {
                let url = item
                    .get("image_url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ProtocolError::invalid("input_image.image_url is required"))?;
                result.push(json!({"type": "image_url", "image_url": {"url": url}}));
            }
            Some(kind) => {
                return Err(ProtocolError::unsupported(format!(
                    "Responses content type {kind} is not supported"
                )))
            }
            None => return Err(ProtocolError::invalid("Responses content type is required")),
        }
    }
    Ok(Value::Array(result))
}

pub fn chat_to_responses(chat: &Value, model: &str) -> Value {
    let response_id = chat
        .get("id")
        .and_then(Value::as_str)
        .map(|id| id.replacen("chatcmpl-", "resp_", 1))
        .unwrap_or_else(|| format!("resp_{}", crate::utils::id_generator::generate_request_id()));
    let message = chat
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = message.get("content").and_then(Value::as_str).unwrap_or("");
    let mut output = vec![json!({
        "id": format!("msg_{}", crate::utils::id_generator::generate_request_id()),
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": []}]
    })];
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            output.push(json!({
                "id": call.get("id").and_then(Value::as_str).unwrap_or(""),
                "type": "function_call",
                "call_id": call.get("id").and_then(Value::as_str).unwrap_or(""),
                "name": call.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
                "arguments": call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("{}"),
                "status": "completed"
            }));
        }
    }
    let input_tokens = chat
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = chat
        .pointer("/usage/completion_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_tokens = chat
        .pointer("/usage/total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(input_tokens + output_tokens);
    json!({
        "id": response_id,
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "status": "completed",
        "error": null,
        "model": model,
        "output": output,
        "parallel_tool_calls": true,
        "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens, "total_tokens": total_tokens}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_preserves_tools_images_and_json_schema() {
        let input = json!({
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "describe"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ]}],
            "tools": [{"type": "function", "function": {"name": "weather", "parameters": {"type": "object"}}}],
            "tool_choice": "required",
            "response_format": {"type": "json_schema", "json_schema": {"name": "answer", "schema": {"type": "object"}}}
        });
        let body = openai_to_anthropic(&input, "claude-test").unwrap();
        assert_eq!(
            body["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(body["tools"][0]["name"], "weather");
        assert_eq!(body["tool_choice"]["type"], "any");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    }

    #[test]
    fn anthropic_response_preserves_tool_call() {
        let response = json!({
            "id": "msg_1", "stop_reason": "tool_use",
            "content": [{"type": "tool_use", "id": "tool_1", "name": "weather", "input": {"city": "Paris"}}],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let output = anthropic_to_openai(&response, "claude");
        assert_eq!(output["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            output["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "weather"
        );
    }

    #[test]
    fn gemini_preserves_tool_round_trip_and_multimodal_input() {
        let input = json!({
            "messages": [
                {"role": "user", "content": [{"type":"image_url","image_url":{"url":"https://example.com/a.png","mime_type":"image/png"}}]},
                {"role": "assistant", "tool_calls": [{"id":"call_1","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}}]},
                {"role": "tool", "tool_call_id":"call_1", "content":"{\"temperature\":20}"}
            ],
            "tools": [{"type":"function","function":{"name":"weather","parameters":{"type":"object"}}}],
            "tool_choice": {"type":"function","function":{"name":"weather"}}
        });
        let body = openai_to_gemini(&input).unwrap();
        assert_eq!(
            body["contents"][0]["parts"][0]["fileData"]["fileUri"],
            "https://example.com/a.png"
        );
        assert_eq!(
            body["contents"][1]["parts"][0]["functionCall"]["name"],
            "weather"
        );
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]["name"],
            "weather"
        );
        assert_eq!(
            body["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
            "weather"
        );
    }

    #[test]
    fn gemini_response_preserves_function_call() {
        let response = json!({
            "candidates": [{"content": {"parts": [{"functionCall": {"id":"call_1","name":"weather","args":{"city":"Paris"}}}]}, "finishReason":"STOP"}],
            "usageMetadata": {"promptTokenCount":10,"candidatesTokenCount":5,"totalTokenCount":15}
        });
        let output = gemini_to_openai(&response, "gemini");
        assert_eq!(output["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            output["choices"][0]["message"]["tool_calls"][0]["id"],
            "call_1"
        );
    }

    #[test]
    fn unsupported_provider_parameter_fails_loudly() {
        let input = json!({"messages": [], "seed": 7});
        let error = openai_to_anthropic(&input, "claude").unwrap_err();
        assert_eq!(error.code, "UNSUPPORTED_PARAMETER");
    }

    #[test]
    fn responses_adapter_preserves_image_tool_and_schema() {
        let input = json!({
            "model": "gpt-test",
            "instructions": "be exact",
            "input": [{"type":"message","role":"user","content":[
                {"type":"input_text","text":"describe"},
                {"type":"input_image","image_url":"https://example.com/a.png"}
            ]}],
            "tools": [{"type":"function","name":"weather","parameters":{"type":"object"}}],
            "text": {"format":{"type":"json_schema","name":"answer","schema":{"type":"object"}}}
        });
        let chat = responses_to_chat(&input).unwrap();
        assert_eq!(chat["messages"][0]["role"], "system");
        assert_eq!(chat["messages"][1]["content"][1]["type"], "image_url");
        assert_eq!(chat["tools"][0]["function"]["name"], "weather");
        assert_eq!(chat["response_format"]["type"], "json_schema");
    }

    #[test]
    fn chat_response_converts_to_responses_shape() {
        let chat = json!({
            "id":"chatcmpl_1",
            "choices":[{"message":{"role":"assistant","content":"ok","tool_calls":[{"id":"call_1","type":"function","function":{"name":"weather","arguments":"{}"}}]}}],
            "usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}
        });
        let response = chat_to_responses(&chat, "gpt-test");
        assert_eq!(response["object"], "response");
        assert_eq!(response["output"][0]["content"][0]["text"], "ok");
        assert_eq!(response["output"][1]["type"], "function_call");
        assert_eq!(response["usage"]["total_tokens"], 3);
    }
}

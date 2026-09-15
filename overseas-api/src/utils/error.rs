/// 将数据库/系统错误转换为用户友好的错误消息，避免把内部细节暴露给前端。
pub fn sanitize_error(e: impl std::error::Error) -> String {
    tracing::error!(error = %e, "Internal operation failed");
    "Internal server error. Please try again later.".to_string()
}

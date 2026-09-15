-- Make api_key_id nullable (JWT auth doesn't use API keys)
ALTER TABLE api_calls ALTER COLUMN api_key_id DROP NOT NULL;

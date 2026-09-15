\set ON_ERROR_STOP on

-- 用法：
-- psql -U postgres -v app_password='替换成强密码' -f deploy/init-postgres.sql
-- 可选：-v db_name='abroad_prod' -v app_user='overseas_app'

\if :{?db_name}
\else
\set db_name abroad_prod
\endif

\if :{?app_user}
\else
\set app_user overseas_app
\endif

\if :{?app_password}
\else
\echo '缺少 app_password。示例：psql -U postgres -v app_password=''替换成强密码'' -f deploy/init-postgres.sql'
\quit 1
\endif

SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'app_user', :'app_password')
WHERE NOT EXISTS (
    SELECT 1 FROM pg_roles WHERE rolname = :'app_user'
)
\gexec

ALTER ROLE :"app_user" WITH LOGIN PASSWORD :'app_password';

SELECT format('CREATE DATABASE %I OWNER %I ENCODING ''UTF8''', :'db_name', :'app_user')
WHERE NOT EXISTS (
    SELECT 1 FROM pg_database WHERE datname = :'db_name'
)
\gexec

\connect :db_name

\ir ../overseas-api/migrations/001_init.sql
\ir ../overseas-api/migrations/002_seed_basic_data.sql
\ir ../overseas-api/migrations/003_add_login_lock_fields.sql
\ir ../overseas-api/migrations/004_alter_api_calls_nullable.sql
\ir ../overseas-api/migrations/005_seed_domestic_providers.sql
\ir ../overseas-api/migrations/006_add_model_pricing_margin.sql
\ir ../overseas-api/migrations/007_add_provider_ops_and_audit.sql
\ir ../overseas-api/migrations/008_harden_billing_and_payments.sql
\ir ../overseas-api/migrations/009_add_aggregation_core.sql
\ir ../overseas-api/migrations/010_add_workspaces_projects_byok.sql
\ir ../overseas-api/migrations/011_disable_unverified_domestic_providers.sql
\ir ../overseas-api/migrations/012_correct_price_scale.sql
\ir ../overseas-api/migrations/013_add_atomic_budget_reservations.sql
\ir ../overseas-api/migrations/014_add_user_token_version.sql
\ir ../overseas-api/migrations/015_add_async_inference_tasks.sql
\ir ../overseas-api/migrations/016_add_replicate_async_provider.sql
\ir ../overseas-api/migrations/017_add_openai_embeddings.sql

GRANT CONNECT ON DATABASE :"db_name" TO :"app_user";
GRANT USAGE, CREATE ON SCHEMA public TO :"app_user";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO :"app_user";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO :"app_user";

ALTER DEFAULT PRIVILEGES IN SCHEMA public
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO :"app_user";

ALTER DEFAULT PRIVILEGES IN SCHEMA public
GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO :"app_user";

SELECT '海外站 PostgreSQL 初始化完成' AS result;

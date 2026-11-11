set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

prometheus_tool_image := "prom/prometheus:v3.9.1@sha256:1f0f50f06acaceb0f5670d2c8a658a599affe7b0d8e78b898c1035653849a702"
alertmanager_tool_image := "prom/alertmanager:v0.28.1@sha256:27c475db5fb156cab31d5c18a4251ac7ed567746a2483ff264516437a39b15ba"

# Validate Compose, Prometheus, alert rules, and Alertmanager configuration.
check env_file=".env.production":
  scripts/production-stack.sh config {{quote(env_file)}} --quiet
  docker run --rm --entrypoint promtool -v "$PWD/docker/prometheus:/etc/prometheus:ro" {{quote(prometheus_tool_image)}} check config /etc/prometheus/prometheus.production.yml
  docker run --rm --entrypoint promtool -v "$PWD/docker/prometheus:/etc/prometheus:ro" {{quote(prometheus_tool_image)}} test rules /etc/prometheus/tests/sol-validator-watcher.test.yml
  docker run --rm --entrypoint amtool -v "$PWD/docker/alertmanager:/etc/alertmanager:ro" {{quote(alertmanager_tool_image)}} check-config /etc/alertmanager/alertmanager.yml

# Deliver a synthetic alert through the committed Alertmanager route.
test-alert-delivery:
  scripts/test-alertmanager-delivery.sh {{quote(alertmanager_tool_image)}}

# Run a five-successful-poll canary without replacing the production service.
canary env_file candidate:
  scripts/canary-production.sh {{quote(env_file)}} {{quote(candidate)}}

# Persist a canary-approved versioned image digest and roll it out with automatic restore on failure.
rollout env_file candidate:
  scripts/rollout-production.sh {{quote(env_file)}} {{quote(candidate)}}

# Restore the image retained by the latest rollout.
rollback env_file=".env.production":
  scripts/rollback-production.sh {{quote(env_file)}}

# Validate and render the versioned, digest-pinned production stack.
config env_file=".env.production":
  scripts/production-stack.sh config {{quote(env_file)}}

# Pull and start the versioned, digest-pinned production stack, then wait for health.
up env_file=".env.production":
  scripts/production-stack.sh up {{quote(env_file)}}

# Stop the production stack without deleting persistent volumes.
down env_file=".env.production":
  scripts/production-stack.sh down {{quote(env_file)}}

# Show production service state.
ps env_file=".env.production":
  scripts/production-stack.sh ps {{quote(env_file)}}

# Wait for the production watcher readiness contract.
ready env_file=".env.production":
  scripts/production-stack.sh ready {{quote(env_file)}}

# Follow production service logs.
logs env_file=".env.production":
  scripts/production-stack.sh logs {{quote(env_file)}} --follow

# Reload the running production Prometheus configuration.
reload-prometheus env_file=".env.production":
  scripts/production-stack.sh reload-prometheus {{quote(env_file)}}

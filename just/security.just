set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

doctor:
  mise exec -- cargo deny --version
  mise exec -- trivy --version

# Audit advisories, licenses, bans, and dependency sources.
dependencies:
  mise exec -- cargo deny check --deny warnings

# Scan repository dependencies, secrets, and infrastructure configuration.
repository:
  mise exec -- trivy fs --db-repository ghcr.io/aquasecurity/trivy-db:2 --scanners vuln,secret,misconfig --severity HIGH,CRITICAL --ignore-unfixed --exit-code 1 --no-progress --skip-dirs target --skip-dirs coverage --skip-files docker/Dockerfile.dockerignore .

# Scan the exact locally available container image before publication.
image image:
  mise exec -- trivy image --db-repository ghcr.io/aquasecurity/trivy-db:2 --scanners vuln,secret --severity HIGH,CRITICAL --ignore-unfixed --exit-code 1 --no-progress {{quote(image)}}

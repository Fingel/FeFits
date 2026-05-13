fixtures_url := "https://github.com/Fingel/FeFits/releases/download/integration-tests/fixtures.tar.xz"
fixtures_archive := "tests/fixtures.tar.xz"
fixtures_dir := "tests/fixtures"

test:
    cargo test

test-integration: fetch-test-data
    cargo test --features integration

# Download and extract integration test fixtures
fetch-test-data:
    mkdir -p {{ fixtures_dir }}
    [ -f {{ fixtures_archive }} ] || curl -L --fail -o {{ fixtures_archive }} {{ fixtures_url }}
    tar -xJf {{ fixtures_archive }} -C {{ fixtures_dir }}

# Remove downloaded test fixtures
clean-test-data:
    rm -rf {{ fixtures_dir }} {{ fixtures_archive }}

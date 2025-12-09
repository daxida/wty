# Add a word to the testsuite
add fr to word:
  @cargo run --release -- download {{fr}} {{to}}
  @sh -c 'if [ "{{to}}" != "en" ]; then \
    rg "\"word\": \"{{word}}\"" "data/kaikki/{{to}}-extract.jsonl" -N | \
    jq -c "select(.word == \"{{word}}\")" \
    >> "tests/kaikki/{{fr}}-{{to}}-extract.jsonl"; \
  else \
    rg "\"word\": \"{{word}}\"" "data/kaikki/{{fr}}-{{to}}-extract.jsonl" -N | \
    jq -c "select(.word == \"{{word}}\")" \
    >> "tests/kaikki/{{fr}}-{{to}}-extract.jsonl"; \
  fi'

# Bench and log. To bench run 'cargo bench'
bench-log:
  @rm -rf target/criterion # remove cache comparisons when logging
  @cargo bench --bench benchmark > "benches/log.txt"

# Benchmarks

## Performance Targets

| Metric | Target | Status |
|--------|--------|--------|
| Message bus throughput | >5,000 events/sec | Pending |
| Inter-agent latency | <50ms | Pending |
| Tx parsing throughput | >10,000 tx/sec | Pending |

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench --workspace

# Run specific benchmark
cargo bench -p messaging
cargo bench -p market-data-agent

# Via script
./scripts/benchmark.sh
```

## Benchmark Suites

### Message Bus (`messaging`)
- `publish_1_subscriber` — Publish throughput with a single subscriber
- `publish_4_subscribers` — Publish throughput with 4 subscribers
- `publish_recv_roundtrip` — Full publish → receive cycle latency

### Transaction Parser (`market-data-agent`)
- `parse_uni_v2_swap` — Full Uniswap V2 swap calldata parsing
- `is_swap_check` — Selector-only swap detection
- `parse_non_swap` — Rejection path for non-swap transactions

## Results

Run `cargo bench` to populate. Detailed HTML reports in `target/criterion/`.

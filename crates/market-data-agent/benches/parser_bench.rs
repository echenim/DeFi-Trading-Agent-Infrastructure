use criterion::{criterion_group, criterion_main, Criterion, Throughput};

use common::types::TxHash;
use market_data_agent::parser::TxParser;
use market_data_agent::rpc::RawTransaction;

fn make_uni_v2_tx() -> RawTransaction {
    let mut data = Vec::new();

    // swapExactTokensForTokens selector
    data.extend_from_slice(&[0x38, 0xed, 0x17, 0x38]);

    // amountIn
    let mut amount_in = [0u8; 32];
    amount_in[16..32].copy_from_slice(&1_000_000_000_000_000_000u128.to_be_bytes());
    data.extend_from_slice(&amount_in);

    // amountOutMin
    let mut amount_out_min = [0u8; 32];
    amount_out_min[16..32].copy_from_slice(&990_000_000_000_000_000u128.to_be_bytes());
    data.extend_from_slice(&amount_out_min);

    // offset to path array
    let mut offset = [0u8; 32];
    offset[31] = 0xa0;
    data.extend_from_slice(&offset);

    // to
    data.extend_from_slice(&[0u8; 32]);
    // deadline
    data.extend_from_slice(&[0u8; 32]);

    // path length = 2
    let mut len = [0u8; 32];
    len[31] = 2;
    data.extend_from_slice(&len);

    // path[0] = token_in
    let mut in_word = [0u8; 32];
    in_word[12..32].copy_from_slice(&[0x11; 20]);
    data.extend_from_slice(&in_word);

    // path[1] = token_out
    let mut out_word = [0u8; 32];
    out_word[12..32].copy_from_slice(&[0x22; 20]);
    data.extend_from_slice(&out_word);

    RawTransaction {
        hash: TxHash([0xAA; 32]),
        from: [0xBB; 20],
        to: Some([0xCC; 20]),
        value: 0,
        input: data,
        gas_price: 20_000_000_000,
    }
}

fn bench_parse_swap(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_parser");
    group.throughput(Throughput::Elements(1));

    let tx = make_uni_v2_tx();

    group.bench_function("parse_uni_v2_swap", |b| {
        b.iter(|| {
            TxParser::parse_swap(&tx)
        });
    });

    group.bench_function("is_swap_check", |b| {
        b.iter(|| {
            TxParser::is_swap(&tx.input)
        });
    });

    // Benchmark with non-swap data
    let non_swap_tx = RawTransaction {
        hash: TxHash([0; 32]),
        from: [0; 20],
        to: Some([0; 20]),
        value: 0,
        input: vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00],
        gas_price: 0,
    };

    group.bench_function("parse_non_swap", |b| {
        b.iter(|| {
            TxParser::parse_swap(&non_swap_tx)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_parse_swap);
criterion_main!(benches);

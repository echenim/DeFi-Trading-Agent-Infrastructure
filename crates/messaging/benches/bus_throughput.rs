use criterion::{criterion_group, criterion_main, Criterion, Throughput};

use common::messages::{Envelope, MarketSignal, Message, SignalType};
use messaging::MessageBus;

fn make_signal() -> Message {
    Message::Signal(Envelope::new(MarketSignal {
        signal_type: SignalType::NewBlock { block_number: 42 },
        quotes: vec![],
        source_tx: None,
    }))
}

fn bench_publish_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("message_bus");
    group.throughput(Throughput::Elements(1));

    group.bench_function("publish_1_subscriber", |b| {
        let bus = MessageBus::new(8192);
        let _sub = bus.subscribe();
        let msg = make_signal();

        b.iter(|| {
            bus.publish(msg.clone()).unwrap();
        });
    });

    group.bench_function("publish_4_subscribers", |b| {
        let bus = MessageBus::new(8192);
        let _subs: Vec<_> = (0..4).map(|_| bus.subscribe()).collect();
        let msg = make_signal();

        b.iter(|| {
            bus.publish(msg.clone()).unwrap();
        });
    });

    group.bench_function("publish_recv_roundtrip", |b| {
        let bus = MessageBus::new(8192);
        let mut sub = bus.subscribe();
        let msg = make_signal();

        b.iter(|| {
            rt.block_on(async {
                bus.publish(msg.clone()).unwrap();
                sub.recv().await.unwrap();
            });
        });
    });

    group.finish();
}

criterion_group!(benches, bench_publish_throughput);
criterion_main!(benches);

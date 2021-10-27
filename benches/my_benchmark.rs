use criterion::{criterion_group, criterion_main, Criterion};
use hellotokio::network;
use tokio::io::BufReader;

fn reader_from_string(text: &str) -> network::Reader<BufReader<&[u8]>> {
    network::Reader::new(BufReader::new(text.as_bytes()))
}

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("read_command", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| {
                let mut stream = reader_from_string("*2\r\n$4PING\r\n$11hello world\r\n");
                async move { network::read_command(&mut stream).await }
            })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

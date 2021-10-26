use buffer::RingBufReader;
use std::mem;
use tokio::io::{self, AsyncWriteExt, BufWriter};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::runtime::Builder;

mod buffer;
mod network;

async fn process_socket(stream: &mut TcpStream) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let (read_stream, write_stream) = stream.split();
    let mut reader: RingBufReader<_, 8196> = RingBufReader::new(read_stream);
    let mut writer = BufWriter::new(write_stream);
    loop {
        network::read_command(&mut reader).await?; // TODO: don't throw away the command!
        writer.write_all(b"+OK\r\n").await?;
        if reader.is_empty() {
            writer.flush().await?;
        }
    }
}

async fn main_loop() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    println!("Listening on port 6379...");

    loop {
        let (mut socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            let fut = process_socket(&mut socket);
            match fut.await {
                Ok(_) => {}
                Err(_) => {} // TODO: don't ignore all errors!
            }
        });
    }
}

// #[tokio::main]
async fn main_async() {
    tokio::spawn(async {
        let fut = main_loop();
        let size = mem::size_of_val(&fut);
        println!("Size of main future: {}", size);
        fut.await.unwrap();
    })
    .await
    .unwrap();
}

fn main() {
    Builder::new_multi_thread()
        .enable_io()
        .build()
        .unwrap()
        .block_on(main_async());
    // To run single-threaded:
    // Builder::new_current_thread()
    //     .enable_io()
    //     .build()
    //     .unwrap()
    //     .block_on(main_async());
}

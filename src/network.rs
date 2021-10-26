use crate::buffer::RingBufReader;
use smallvec::{smallvec, SmallVec};
use std::convert::TryFrom;
use tokio::io::{self, AsyncRead};

pub type ArgsVec = SmallVec<[RedisValue; 3]>;
pub type StringVec = SmallVec<[u8; 16]>;
pub type Reader<R> = RingBufReader<R, 8196>;

#[derive(Clone, Debug, PartialEq)]
pub enum RedisValue {
    Array(Vec<RedisValue>),
    SimpleString(StringVec),
    String(StringVec),
    Integer(i64),
    Error(StringVec),
    Null,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RedisRequest {
    pub command: StringVec,
    pub args: ArgsVec,
}

#[derive(Clone, Debug, PartialEq)]
enum AnyVec {
    Small(ArgsVec),
    Normal(Vec<RedisValue>),
}

#[derive(Clone, Debug, PartialEq)]
struct PartialArray {
    length: usize,
    items: AnyVec,
}

#[derive(Clone, Debug, PartialEq)]
enum ReadValue {
    Complete(RedisValue),
    Partial(PartialArray),
}

async fn read_integer<T: AsyncRead + Unpin>(reader: &mut Reader<T>) -> io::Result<i64> {
    let mut sign: i64 = 1;
    let mut value: i64 = 0;

    let c = reader.read_u8().await?;
    if c == b'-' {
        sign = -1;
    } else if c >= b'0' && c <= b'9' {
        value = (c - b'0').into();
    }

    for _ in 0..20 {
        let c = reader.read_u8().await?;
        if c == b'\r' {
            let c = reader.read_u8().await?;
            if c == b'\n' {
                return Ok(value);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid integer (unexpected line ending)",
            ));
        }
        value = match value.checked_mul(10) {
            Some(x) => x,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Too large integer",
                ))
            }
        };
        if c >= b'0' && c <= b'9' {
            let digit: i64 = (c - b'0').into();
            value = match value.checked_add(sign * digit) {
                Some(x) => x,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Too large integer",
                    ))
                }
            };
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid integer (non-digit character)",
            ));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "Invalid integer",
    ))
}

async fn read_usize<T: AsyncRead + Unpin>(reader: &mut Reader<T>) -> io::Result<usize> {
    match usize::try_from(read_integer(reader).await?) {
        Ok(x) => Ok(x),
        Err(_) => Err(io::Error::new(io::ErrorKind::InvalidInput, "Negative size")),
    }
}

async fn read_tag<T: AsyncRead + Unpin>(
    reader: &mut Reader<T>,
    expected_value: u8,
) -> io::Result<()> {
    let tag = reader.read_u8().await?;
    if tag != expected_value {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Expected {} at start, but got {}",
                expected_value as char, tag as char
            ),
        ));
    }
    Ok(())
}

async fn read_crlf<T: AsyncRead + Unpin>(reader: &mut Reader<T>) -> io::Result<()> {
    read_tag(reader, b'\r').await?;
    read_tag(reader, b'\n').await?;
    Ok(())
}

async fn read_bulk_string<T: AsyncRead + Unpin>(reader: &mut Reader<T>) -> io::Result<StringVec> {
    let length = read_usize(reader).await?;
    let mut buf = SmallVec::with_capacity(length);
    reader.read_exact(&mut buf, length).await?;
    read_crlf(reader).await?;

    Ok(buf)
}

async fn read_bulk_array_start<T: AsyncRead + Unpin>(
    reader: &mut Reader<T>,
) -> io::Result<PartialArray> {
    let length = read_usize(reader).await?;

    Ok(PartialArray {
        length,
        items: AnyVec::Normal(Vec::with_capacity(length)),
    })
}

async fn read_redis_value<T: AsyncRead + Unpin>(reader: &mut Reader<T>) -> io::Result<ReadValue> {
    let tag = reader.read_u8().await?;
    match tag {
        b'$' => Ok(ReadValue::Complete(RedisValue::String(
            read_bulk_string(reader).await?,
        ))),
        b':' => Ok(ReadValue::Complete(RedisValue::Integer(
            read_integer(reader).await?,
        ))),
        b'*' => Ok(ReadValue::Partial(read_bulk_array_start(reader).await?)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid argument",
        )),
    }
}

async fn read_bulk_array<T: AsyncRead + Unpin>(
    reader: &mut Reader<T>,
    length: usize,
) -> io::Result<ArgsVec> {
    let mut stack: SmallVec<[_; 1]> = smallvec![PartialArray {
        length,
        items: AnyVec::Small(SmallVec::with_capacity(length)),
    }];

    loop {
        let length = stack.last().unwrap().length;
        let items = match &stack.last().unwrap().items {
            AnyVec::Small(items) => items.len(),
            AnyVec::Normal(items) => items.len(),
        };
        if items == length {
            if stack.len() == 1 {
                return match stack.pop().unwrap().items {
                    AnyVec::Small(array) => Ok(array),
                    AnyVec::Normal(_) => panic!("Unexpected normal Vec"),
                };
            }
            if let AnyVec::Normal(finished_array) = stack.pop().unwrap().items {
                match &mut stack.last_mut().unwrap().items {
                    AnyVec::Normal(items) => items.push(RedisValue::Array(finished_array)),
                    AnyVec::Small(items) => items.push(RedisValue::Array(finished_array)),
                }
            } else {
                panic!("Unexpected small Vec");
            }
        } else {
            match read_redis_value(reader).await? {
                ReadValue::Complete(value) => match &mut stack.last_mut().unwrap().items {
                    AnyVec::Normal(items) => items.push(value),
                    AnyVec::Small(items) => items.push(value),
                },
                ReadValue::Partial(new_array) => {
                    stack.push(new_array);
                }
            }
        }
    }
}

async fn read_bulk_command<T: AsyncRead + Unpin>(
    reader: &mut Reader<T>,
) -> io::Result<RedisRequest> {
    let num_args = read_usize(reader).await?;
    if num_args < 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "No command specified!",
        ));
    }
    read_tag(reader, b'$').await?;

    Ok(RedisRequest {
        command: read_bulk_string(reader).await?,
        args: read_bulk_array(reader, num_args - 1).await?,
    })
}

struct InlineString {
    text: StringVec,
    last: bool,
}

async fn read_inline_string<T: AsyncRead + Unpin>(
    reader: &mut Reader<T>,
) -> io::Result<InlineString> {
    let mut buf = smallvec![];
    loop {
        let c = reader.read_u8().await?;
        if c == b' ' {
            return Ok(InlineString {
                text: buf,
                last: false,
            });
        } else if c == b'\n' {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unexpected linefeed",
            ));
        } else if c == b'\r' {
            read_tag(reader, b'\n').await?;
            return Ok(InlineString {
                text: buf,
                last: true,
            });
        } else {
            buf.push(c);
        }
    }
}

async fn read_inline_command<T: AsyncRead + Unpin>(
    reader: &mut Reader<T>,
    first_byte: u8,
) -> io::Result<RedisRequest> {
    if first_byte == b'\r' || first_byte == b'\n' {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Empty command"));
    }
    let mut command = read_inline_string(reader).await?;
    command.text.insert(0, first_byte);
    let mut request = RedisRequest {
        command: command.text,
        args: smallvec![],
    };
    loop {
        let arg = read_inline_string(reader).await?;
        request.args.push(RedisValue::String(arg.text));
        if arg.last {
            return Ok(request);
        }
    }
}

pub async fn read_command<T: AsyncRead + Unpin>(
    reader: &mut Reader<T>,
) -> io::Result<RedisRequest> {
    let tag = reader.read_u8().await?;
    match tag {
        b'*' => read_bulk_command(reader).await,
        _ => read_inline_command(reader, tag).await,
    }
}

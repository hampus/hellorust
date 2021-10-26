# Async Tokio Server Experiment

This is a small experimental Rust server that implements a very primitive
prototype of the Redis protocol and can reply OK to commands. It's functionally
extremely primitive (and without even unit testing), but it has been profiled
and highly optimized. The whole point is to learn more Rust and check out how
easy it is to achieve similar performance to C and C++.

Run it with `cargo run --release`.

It's possible to run benchmarks using official Redis, such as:

    $ redis-benchmark -t set -n 10000000 -c 100 -P 64 -q
    ERROR: failed to fetch CONFIG from 127.0.0.1:6379
    WARN: could not fetch server CONFIG
    SET: 3749531.25 requests per second, p50=0.863 msec 

The error and warning is due to missing functionality, which wasn't needed for
giving the performance optimization a try.

To optimize, flame graphs were used.

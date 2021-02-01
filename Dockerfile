FROM rust:alpine:alpine3.12 AS rust-builder

WORKDIR /code
COPY . /code

RUN rustup toolchain install nightly
RUN cargo +nightly build --release

FROM alpine:3.12

WORKDIR /code
COPY --from=rust-builder /code/target/release/cosmwasm-simulate /usr/bin/cosmwasm-simulate
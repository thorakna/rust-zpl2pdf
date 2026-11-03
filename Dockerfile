# Builds the zpl2pdf HTTP service.
#   docker build -t zpl2pdf .
#   docker run --rm -p 8080:8080 zpl2pdf
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p zpl2pdf-server -p zpl2pdf-cli

FROM debian:bookworm-slim
COPY --from=build /src/target/release/zpl2pdf-server /usr/local/bin/zpl2pdf-server
COPY --from=build /src/target/release/zpl2pdf /usr/local/bin/zpl2pdf
ENV ZPL2PDF_LISTEN=0.0.0.0:8080
EXPOSE 8080
USER 65534:65534
ENTRYPOINT ["/usr/local/bin/zpl2pdf-server"]

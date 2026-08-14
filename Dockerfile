FROM rust:1.97.1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo generate-lockfile \
    && cargo build --workspace --release --all-features --locked

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 cordis
COPY --from=build /src/target/release/cordis /usr/local/bin/cordis
COPY --from=build /src/target/release/cordis-mcp /usr/local/bin/cordis-mcp
USER cordis
WORKDIR /home/cordis
ENTRYPOINT ["cordis-mcp"]
CMD ["--data-dir", ".cordis"]

FROM rust:slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends g++ \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
LABEL io.modelcontextprotocol.server.name="io.github.balaianu/cogz"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/cogz /usr/local/bin/cogz
ENTRYPOINT ["cogz", "mcp-stdio"]

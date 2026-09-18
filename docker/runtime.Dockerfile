# syntax=docker/dockerfile:1.7
# sakaladev/usai:<version> — the runtime only: the `usai` binary, CA roots
# for `tls.caFile`, a non-root user. No Node, no compiler, no source tree:
# it serves an application artifact built elsewhere (`usai build`, typically
# in the `-dev` image). See docker/dev.Dockerfile and docs/GUIDE.md.

FROM rust:1.98-bookworm AS build
WORKDIR /src
COPY . .
# The vendored Wasmtime (vendor/) and the guest core are part of the tree;
# nothing is fetched from the research repository.
# A release build uses the exact binary published on the GitHub release
# (placed at prebuilt/usai by the workflow), so the image and the tarball
# carry identical bits; a local build compiles from the tree.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    if [ -x prebuilt/usai ]; then \
      install -m755 prebuilt/usai /usr/local/bin/usai; \
    else \
      cargo build --profile dist -p usai-cli --locked \
      && install -m755 target/dist/usai /usr/local/bin/usai; \
    fi

FROM debian:bookworm-slim
ARG USAI_VERSION=dev
LABEL org.opencontainers.image.title="Usai runtime" \
      org.opencontainers.image.description="Lifecycle-native application runtime: serves a Usai application artifact." \
      org.opencontainers.image.source="https://github.com/gmedia/usai" \
      org.opencontainers.image.version="${USAI_VERSION}" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.vendor="Sakala maintainers"
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --gid 10001 usai \
 && useradd --uid 10001 --gid 10001 --create-home --home-dir /home/usai --shell /usr/sbin/nologin usai \
 && mkdir -p /app && chown usai:usai /app
COPY --from=build /usr/local/bin/usai /usr/local/bin/usai
USER usai
WORKDIR /app
# Production configuration comes from the environment, never from .env.
# A precompiled artifact needs no compilation cache; the filesystem may be
# read-only.
ENV USAI_COMPILE_CACHE=0 \
    HOME=/home/usai
EXPOSE 3000
ENTRYPOINT ["usai"]
CMD ["run", "--artifact", "/app/.usai/build", "--host", "0.0.0.0", "--port", "3000"]

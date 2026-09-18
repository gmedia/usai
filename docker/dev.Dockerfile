# syntax=docker/dockerfile:1.7
# sakaladev/usai:<version>-dev — the runtime binary plus what building an
# application needs: Node 24, pnpm, git. For `usai dev`, `usai build`,
# `usai test` without installing anything locally, and as the build stage of
# an application image (see the scaffold's Dockerfile).
ARG USAI_RUNTIME_IMAGE=sakaladev/usai:dev
FROM ${USAI_RUNTIME_IMAGE} AS runtime

FROM node:24-bookworm-slim
ARG USAI_VERSION=dev
LABEL org.opencontainers.image.title="Usai dev" \
      org.opencontainers.image.description="Usai runtime plus Node 24, pnpm and git: build, develop and test Usai applications." \
      org.opencontainers.image.source="https://github.com/gmedia/usai" \
      org.opencontainers.image.version="${USAI_VERSION}" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.vendor="Sakala maintainers"
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates git curl \
 && rm -rf /var/lib/apt/lists/* \
 && npm install -g pnpm@12.3.4 \
 && npm cache clean --force \
 && mkdir -p /app/node_modules && chown -R node:node /app
COPY --from=runtime /usr/local/bin/usai /usr/local/bin/usai
# /app/node_modules exists and is owned by `node` so a named volume mounted
# there (the scaffold's compose.yaml) inherits that ownership.
USER node
WORKDIR /app
ENV HOME=/home/node
EXPOSE 3000
# Same convention as the runtime image: the container *is* the usai command.
# `docker run --rm -it --entrypoint sh sakaladev/usai:<v>-dev` for a shell.
ENTRYPOINT ["usai"]
CMD ["dev", "--host", "0.0.0.0", "--port", "3000"]

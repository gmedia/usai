# Two stages, one artifact: the build stage (Node, pnpm, the SDK) produces
# the application artifact; the final image is the Usai runtime plus that
# artifact and nothing else — no Node, no source tree, non-root.
FROM sakaladev/usai:__USAI_IMAGE_TAG__-dev AS build
WORKDIR /app
COPY package.json pnpm-lock.yaml* pnpm-workspace.yaml* ./
# Frozen when a lockfile is committed (always, once the project is real).
RUN --mount=type=cache,target=/home/node/.local/share/pnpm/store,uid=1000 \
    if [ -f pnpm-lock.yaml ]; then pnpm install --frozen-lockfile; else pnpm install; fi
COPY . .
RUN usai build

FROM sakaladev/usai:__USAI_IMAGE_TAG__
WORKDIR /app
COPY --from=build --chown=usai:usai /app/.usai/build /app/.usai/build
# Configuration comes from the environment (DATABASE_URL, …), never from .env.
EXPOSE 3000
CMD ["run", "--artifact", "/app/.usai/build", "--host", "0.0.0.0", "--port", "3000"]

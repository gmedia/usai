# The invoicing example as a production image: artifact built with the dev
# image from the checkout (workspace SDK), served by the runtime image.
ARG USAI_IMAGE=sakaladev/usai:p5
FROM ${USAI_IMAGE}
COPY --chown=usai:usai build /app/.usai/build

# php-fpm with pdo_pgsql built in (installing at container start is slow and
# needs build tools); the compose file uses this image when built.
#
# Two profiles (build arg PHP_PROFILE):
#   shipped — the official image as it comes: php.ini-production, opcache
#             NOT enabled (the extension is built but not loaded), FPM pool
#             default (pm = dynamic, max_children = 5). What a team that
#             copies the Dockerfile from the image's README runs.
#   tuned   — opcache on with the tracing JIT, no timestamp validation
#             (files compile once), realpath cache, FPM pm = static with
#             PHP_CHILDREN workers. What a tuned deployment looks like before
#             a framework enters the picture. No preload: index.php has
#             top-level code, and the gain over opcache alone is small.
FROM php:8.4-fpm-alpine
ARG PHP_PROFILE=shipped
ARG PHP_CHILDREN=32
RUN apk add --no-cache postgresql-dev && docker-php-ext-install pdo_pgsql
RUN mv "$PHP_INI_DIR/php.ini-production" "$PHP_INI_DIR/php.ini" && echo "display_errors=Off" > "$PHP_INI_DIR/conf.d/usai-bench.ini"
RUN if [ "$PHP_PROFILE" = tuned ]; then \
      docker-php-ext-enable opcache \
   && printf '%s\n' \
        'opcache.enable=1' 'opcache.enable_cli=0' 'opcache.jit=tracing' 'opcache.jit_buffer_size=64M' \
        'opcache.memory_consumption=128' 'opcache.max_accelerated_files=4000' 'opcache.validate_timestamps=0' \
        'realpath_cache_size=4096K' 'realpath_cache_ttl=600' \
        > "$PHP_INI_DIR/conf.d/zz-tuned.ini" \
   && printf '%s\n' '[www]' 'pm = static' "pm.max_children = $PHP_CHILDREN" 'pm.max_requests = 0' \
        > /usr/local/etc/php-fpm.d/zz-tuned.conf; \
    fi

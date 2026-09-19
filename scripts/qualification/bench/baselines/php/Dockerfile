# php-fpm with pdo_pgsql built in (installing at container start is slow and
# needs build tools); the compose file uses this image when built.
FROM php:8.4-fpm-alpine
RUN apk add --no-cache postgresql-dev && docker-php-ext-install pdo_pgsql
RUN mv "$PHP_INI_DIR/php.ini-production" "$PHP_INI_DIR/php.ini" && echo "display_errors=Off" > "$PHP_INI_DIR/conf.d/usai-bench.ini"

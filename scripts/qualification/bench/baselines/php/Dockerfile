# php-fpm with pdo_pgsql built in (installing at container start is slow and
# needs build tools); the compose file uses this image when built.
FROM php:8.4-fpm-alpine
RUN apk add --no-cache postgresql-dev && docker-php-ext-install pdo_pgsql

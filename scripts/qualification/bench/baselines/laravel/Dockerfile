# Laravel on PHP-FPM — the PHP stack teams actually deploy, tuned the way a
# production Laravel is (opcache + JIT, config/route/event caches, pm=static),
# doing the bench app's work with the framework's own pieces: the router,
# Validator rules, the DB facade (query builder + transaction), a middleware
# for the API key. The skeleton comes from `composer create-project` at image
# build (network at build time); only the files under this directory are ours.
FROM composer:2 AS composer
FROM php:8.4-fpm-alpine
ARG PHP_CHILDREN=32
ARG LARAVEL_VERSION=^12.0
# The database URL is frozen into the config cache at build (config:cache
# resolves env() once), so the suite passes it as a build argument too.
ARG DB_URL
RUN apk add --no-cache postgresql-dev git unzip && docker-php-ext-install pdo_pgsql opcache
COPY --from=composer /usr/bin/composer /usr/bin/composer
WORKDIR /var/www
# The base image ships `/var/www/html`, and `create-project .` refuses a
# directory that is not empty.
RUN rm -rf /var/www/html \
 && composer create-project laravel/laravel:"$LARAVEL_VERSION" . --no-interaction --prefer-dist --no-dev --no-scripts --no-progress \
 && rm -rf routes/web.php database/migrations/* \
 && printf '%s\n' '<?php' 'Route::get("/", fn () => response()->json(["ok" => true]));' > routes/web.php
COPY bootstrap/app.php bootstrap/app.php
COPY routes/api.php routes/api.php
COPY app/Http/Controllers/BenchController.php app/Http/Controllers/BenchController.php
COPY app/Http/Middleware/ApiKey.php app/Http/Middleware/ApiKey.php
COPY app/Providers/AppServiceProvider.php app/Providers/AppServiceProvider.php
RUN mv "$PHP_INI_DIR/php.ini-production" "$PHP_INI_DIR/php.ini" \
 && printf '%s\n' \
      'display_errors=Off' \
      'opcache.enable=1' 'opcache.jit=tracing' 'opcache.jit_buffer_size=64M' \
      'opcache.memory_consumption=192' 'opcache.max_accelerated_files=20000' 'opcache.validate_timestamps=0' \
      'realpath_cache_size=4096K' 'realpath_cache_ttl=600' \
      > "$PHP_INI_DIR/conf.d/zz-tuned.ini" \
 && printf '%s\n' '[www]' 'pm = static' "pm.max_children = $PHP_CHILDREN" 'pm.max_requests = 0' 'clear_env = no' \
      > /usr/local/etc/php-fpm.d/zz-tuned.conf
ENV APP_ENV=production APP_DEBUG=false APP_KEY=base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= \
    LOG_CHANNEL=stderr SESSION_DRIVER=array CACHE_STORE=array QUEUE_CONNECTION=sync \
    DB_CONNECTION=pgsql DB_URL=$DB_URL BCRYPT_ROUNDS=4
# The caches a tuned deployment ships with.
RUN php artisan config:cache && php artisan route:cache && php artisan event:cache \
 && chown -R www-data:www-data storage bootstrap/cache

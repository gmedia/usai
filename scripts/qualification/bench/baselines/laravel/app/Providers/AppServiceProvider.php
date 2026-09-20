<?php

namespace App\Providers;

use Illuminate\Support\ServiceProvider;
use PDO;

class AppServiceProvider extends ServiceProvider
{
    public function register(): void
    {
        // Persistent connections, as the plain PHP comparator and most tuned
        // FPM deployments use: one PostgreSQL connection per worker, not per
        // request.
        config(['database.connections.pgsql.options' => [PDO::ATTR_PERSISTENT => true, PDO::ATTR_EMULATE_PREPARES => false]]);
    }

    public function boot(): void {}
}

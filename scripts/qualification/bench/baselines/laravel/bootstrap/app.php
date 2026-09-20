<?php

use App\Http\Middleware\ApiKey;
use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Exceptions;
use Illuminate\Foundation\Configuration\Middleware;
use Illuminate\Http\Request;
use Symfony\Component\HttpKernel\Exception\NotFoundHttpException;

// The bench routes are the API, mounted at the root (no /api prefix), with
// the framework's error envelope replaced by the shared one.
return Application::configure(basePath: dirname(__DIR__))
    ->withRouting(
        web: __DIR__.'/../routes/web.php',
        api: __DIR__.'/../routes/api.php',
        apiPrefix: '',
    )
    ->withMiddleware(function (Middleware $middleware): void {
        $middleware->alias(['api.key' => ApiKey::class]);
    })
    ->withExceptions(function (Exceptions $exceptions): void {
        $exceptions->render(function (NotFoundHttpException $e, Request $request) {
            return response()->json(['error' => ['code' => 'route_not_found', 'message' => 'no route matches '.$request->method().' '.$request->getPathInfo()]], 404);
        });
        $exceptions->render(function (Throwable $e, Request $request) {
            if ($e instanceof \Symfony\Component\HttpKernel\Exception\HttpExceptionInterface) {
                return null;
            }
            return response()->json(['error' => ['code' => 'internal', 'message' => 'internal error']], 500);
        });
    })->create();

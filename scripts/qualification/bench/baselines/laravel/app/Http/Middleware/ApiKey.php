<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

// Class F: the key is looked up in PostgreSQL before the route runs.
class ApiKey
{
    public function handle(Request $request, Closure $next)
    {
        $key = $request->header('x-api-key');
        if ($key === null || $key === '') {
            return response()->json(['error' => ['code' => 'unauthorized', 'message' => 'missing x-api-key header']], 401);
        }
        $row = DB::selectOne('select user_id from api_keys where key = ?', [$key]);
        if ($row === null) {
            return response()->json(['error' => ['code' => 'unauthorized', 'message' => 'unknown_key']], 401);
        }
        $request->attributes->set('userId', (int) $row->user_id);

        return $next($request);
    }
}

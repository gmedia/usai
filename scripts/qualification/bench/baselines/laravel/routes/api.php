<?php

use App\Http\Controllers\BenchController;
use Illuminate\Support\Facades\Route;

Route::get('/health', [BenchController::class, 'health']);
Route::get('/hello/{name}', [BenchController::class, 'hello']);
Route::post('/orders/quote', [BenchController::class, 'quote']);
Route::get('/users/{id}', [BenchController::class, 'user']);
Route::post('/users', [BenchController::class, 'createUser']);
Route::post('/orders/{id}/pay', [BenchController::class, 'pay']);
Route::get('/me', [BenchController::class, 'me'])->middleware('api.key');
Route::get('/counter', [BenchController::class, 'counter']);
Route::get('/slow', [BenchController::class, 'slow']);

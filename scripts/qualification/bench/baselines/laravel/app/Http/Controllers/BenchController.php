<?php

namespace App\Http\Controllers;

use Illuminate\Database\QueryException;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Validator;

// The bench app's six classes with the framework's own tools: Validator
// rules for what Zod checks elsewhere, the DB facade for the SQL, a
// transaction with a row lock for the payment. Same envelope, same statuses.
class BenchController extends Controller
{
    private static int $counter = 0;

    private function invalid(string $slot, array $errors): JsonResponse
    {
        $issues = [];
        foreach ($errors as $path => $messages) {
            $issues[] = ['message' => $messages[0], 'path' => '/'.str_replace('.', '/', $path)];
        }

        return response()->json(['error' => ['code' => 'validation_failed', 'message' => "$slot failed validation", 'details' => ['slot' => $slot, 'issues' => $issues]]], 400);
    }

    private function fail(int $status, string $code, string $message): JsonResponse
    {
        return response()->json(['error' => ['code' => $code, 'message' => $message]], $status);
    }

    private function jsonBody(Request $request): ?array
    {
        $body = json_decode($request->getContent(), true);

        return is_array($body) ? $body : null;
    }

    /** The user row as the shared response contract wants it. Not `user()`:
     * that is the route handler, and PHP has one namespace for both. */
    private function userJson(object $row): array
    {
        return ['id' => (int) $row->id, 'name' => $row->name, 'email' => $row->email];
    }

    public function health(): JsonResponse
    {
        return response()->json(['ok' => true]);
    }

    public function hello(string $name): JsonResponse
    {
        $v = Validator::make(['name' => $name], ['name' => 'required|string|min:1|max:40']);
        if ($v->fails()) {
            return $this->invalid('params', $v->errors()->toArray());
        }

        return response()->json(['hello' => $name]);
    }

    public function quote(Request $request): JsonResponse
    {
        $b = $this->jsonBody($request);
        if ($b === null) {
            return $this->invalid('body', ['' => ['request body is not valid JSON']]);
        }
        $v = Validator::make($b, [
            'customer' => 'required|string|min:1|max:200',
            'email' => 'required|string|email',
            'currency' => 'required|in:USD,EUR,IDR',
            'country' => 'required|string|size:2',
            'priority' => 'sometimes|in:low,normal,high',
            'requestedDate' => 'required|date_format:Y-m-d',
            'reference' => 'required|uuid',
            'tags' => 'present|array|max:10',
            'tags.*' => 'string|max:32',
            'items' => 'required|array|min:1|max:50',
            'items.*.sku' => 'required|string|min:1|max:32',
            'items.*.quantity' => 'required|integer|min:1|max:1000',
            'items.*.unitCents' => 'required|integer|min:0|max:10000000',
        ]);
        if ($v->fails()) {
            return $this->invalid('body', $v->errors()->toArray());
        }
        $subtotal = 0;
        foreach ($b['items'] as $item) {
            $subtotal += $item['quantity'] * $item['unitCents'];
        }
        $rate = $b['country'] === 'ID' ? 11 : ($b['country'] === 'DE' ? 19 : 0);
        $tax = (int) round($subtotal * $rate / 100);

        return response()->json([
            'reference' => $b['reference'],
            'currency' => $b['currency'],
            'subtotalCents' => $subtotal,
            'taxCents' => $tax,
            'totalCents' => $subtotal + $tax,
            'lines' => count($b['items']),
            'priority' => $b['priority'] ?? 'normal',
        ], 201);
    }

    private function parseId(string $raw): int|JsonResponse
    {
        $v = Validator::make(['id' => $raw], ['id' => 'required|integer|min:1|max:2147483647']);
        if ($v->fails()) {
            return $this->invalid('params', $v->errors()->toArray());
        }

        return (int) $raw;
    }

    public function user(string $id): JsonResponse
    {
        $id = $this->parseId($id);
        if ($id instanceof JsonResponse) {
            return $id;
        }
        $row = DB::selectOne('select id, name, email from users where id = ?', [$id]);
        if ($row === null) {
            return $this->fail(404, 'not_found', 'user_not_found');
        }

        return response()->json($this->userJson($row));
    }

    public function createUser(Request $request): JsonResponse
    {
        $b = $this->jsonBody($request);
        if ($b === null) {
            return $this->invalid('body', ['' => ['request body is not valid JSON']]);
        }
        $v = Validator::make($b, ['name' => 'required|string|min:1|max:200', 'email' => 'required|string|email']);
        if ($v->fails()) {
            return $this->invalid('body', $v->errors()->toArray());
        }
        try {
            $row = DB::selectOne('insert into users (name, email) values (?, ?) returning id, name, email', [$b['name'], $b['email']]);
        } catch (QueryException $e) {
            if ($e->getCode() === '23505') {
                return $this->fail(409, 'conflict', 'email_taken');
            }
            throw $e;
        }

        return response()->json($this->userJson($row), 201);
    }

    public function pay(string $id): JsonResponse
    {
        $id = $this->parseId($id);
        if ($id instanceof JsonResponse) {
            return $id;
        }

        return DB::transaction(function () use ($id) {
            $order = DB::selectOne('select id, total_cents, paid from orders where id = ? for update', [$id]);
            if ($order === null) {
                return $this->fail(404, 'not_found', 'order_not_found');
            }
            if ($order->paid) {
                return $this->fail(409, 'conflict', 'already_paid');
            }
            DB::update('update orders set paid = true where id = ?', [$order->id]);
            $payment = DB::selectOne('insert into payments (order_id, amount_cents) values (?, ?) returning id', [$order->id, $order->total_cents]);

            return response()->json(['orderId' => (int) $order->id, 'paymentId' => (int) $payment->id, 'amountCents' => (int) $order->total_cents, 'paid' => true]);
        });
    }

    public function me(Request $request): JsonResponse
    {
        $row = DB::selectOne('select id, name, email from users where id = ?', [$request->attributes->get('userId')]);
        if ($row === null) {
            return $this->fail(404, 'not_found', 'user_not_found');
        }

        return response()->json($this->userJson($row));
    }

    public function counter(): JsonResponse
    {
        // Per FPM worker and reset per request: 1, as for plain PHP.
        return response()->json(['count' => ++self::$counter]);
    }

    public function slow(Request $request): JsonResponse
    {
        $v = Validator::make($request->query(), ['ms' => 'sometimes|integer|min:0|max:30000']);
        if ($v->fails()) {
            return $this->invalid('query', $v->errors()->toArray());
        }
        $ms = (int) ($request->query('ms') ?? 1000);
        usleep($ms * 1000);

        return response()->json(['slept' => $ms]);
    }
}

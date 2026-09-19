<?php
// PHP-FPM comparator (reporting only, c=1): PDO with persistent connections,
// the same routes, validation rules, SQL and error envelope. Served by
// nginx + php-fpm from compose.yaml.
declare(strict_types=1);
header('Content-Type: application/json');
// A failure is a 500 with the shared envelope, never a 200 with an HTML
// error page (display_errors is off in the image; this catches the rest).
set_exception_handler(function (Throwable $e): void {
  error_log($e->getMessage());
  http_response_code(500);
  echo json_encode(['error' => ['code' => 'internal', 'message' => 'internal error']]);
});

function reply(int $status, array $body): never { http_response_code($status); echo json_encode($body); exit; }
function err(int $status, string $code, string $message): never { reply($status, ['error' => ['code' => $code, 'message' => $message]]); }
function invalid(string $slot, string $path, string $message): never {
  reply(400, ['error' => ['code' => 'validation_failed', 'message' => "$slot failed validation", 'details' => ['slot' => $slot, 'issues' => [['message' => $message, 'path' => $path]]]]]);
}
function db(): PDO {
  static $pdo = null;
  if ($pdo === null) {
    $url = parse_url(getenv('DATABASE_URL'));
    $dsn = sprintf('pgsql:host=%s;port=%d;dbname=%s', $url['host'], $url['port'] ?? 5432, ltrim($url['path'], '/'));
    $pdo = new PDO($dsn, $url['user'] ?? null, $url['pass'] ?? null, [PDO::ATTR_PERSISTENT => true, PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION, PDO::ATTR_EMULATE_PREPARES => false]);
  }
  return $pdo;
}
function parse_id(string $raw): int {
  if (!preg_match('/^[0-9]+$/', $raw)) invalid('params', '/id', 'Invalid input: expected number, received NaN');
  $n = (int) $raw;
  if ($n < 1 || $n > 2147483647) invalid('params', '/id', 'range');
  return $n;
}
function user_row(array $row): array { return ['id' => (int) $row['id'], 'name' => $row['name'], 'email' => $row['email']]; }
function json_body(): array {
  $raw = file_get_contents('php://input');
  $body = json_decode($raw === false ? '' : $raw, true);
  if (!is_array($body)) invalid('body', '', 'request body is not valid JSON');
  return $body;
}

$method = $_SERVER['REQUEST_METHOD'];
$path = parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH);

if ($method === 'GET' && $path === '/health') reply(200, ['ok' => true]);
if ($method === 'GET' && preg_match('#^/hello/([^/]+)$#', $path, $m)) {
  $name = rawurldecode($m[1]);
  if ($name === '' || mb_strlen($name) > 40) invalid('params', '/name', 'length');
  reply(200, ['hello' => $name]);
}
if ($method === 'POST' && $path === '/orders/quote') {
  $b = json_body();
  $s = fn(string $k) => is_string($b[$k] ?? null) ? $b[$k] : null;
  if ($s('customer') === null || $b['customer'] === '' || mb_strlen($b['customer']) > 200) invalid('body', '/customer', 'length');
  if ($s('email') === null || !filter_var($b['email'], FILTER_VALIDATE_EMAIL)) invalid('body', '/email', 'Invalid email address');
  if (!in_array($s('currency'), ['USD', 'EUR', 'IDR'], true)) invalid('body', '/currency', 'Invalid option');
  if ($s('country') === null || mb_strlen($b['country']) !== 2) invalid('body', '/country', 'length');
  $priority = $b['priority'] ?? 'normal';
  if (!in_array($priority, ['low', 'normal', 'high'], true)) invalid('body', '/priority', 'Invalid option');
  if ($s('requestedDate') === null || !preg_match('/^\d{4}-\d{2}-\d{2}$/', $b['requestedDate'])) invalid('body', '/requestedDate', 'Invalid string');
  if ($s('reference') === null || !preg_match('/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i', $b['reference'])) invalid('body', '/reference', 'Invalid UUID');
  if (!is_array($b['tags'] ?? null) || count($b['tags']) > 10) invalid('body', '/tags', 'length');
  if (!is_array($b['items'] ?? null) || count($b['items']) < 1 || count($b['items']) > 50) invalid('body', '/items', 'length');
  $subtotal = 0;
  foreach ($b['items'] as $i => $item) {
    if (!is_string($item['sku'] ?? null) || $item['sku'] === '' || strlen($item['sku']) > 32) invalid('body', "/items/$i/sku", 'length');
    if (!is_int($item['quantity'] ?? null) || $item['quantity'] < 1 || $item['quantity'] > 1000) invalid('body', "/items/$i/quantity", 'range');
    if (!is_int($item['unitCents'] ?? null) || $item['unitCents'] < 0 || $item['unitCents'] > 10000000) invalid('body', "/items/$i/unitCents", 'range');
    $subtotal += $item['quantity'] * $item['unitCents'];
  }
  $rate = $b['country'] === 'ID' ? 11 : ($b['country'] === 'DE' ? 19 : 0);
  $tax = (int) round($subtotal * $rate / 100);
  reply(201, ['reference' => $b['reference'], 'currency' => $b['currency'], 'subtotalCents' => $subtotal, 'taxCents' => $tax, 'totalCents' => $subtotal + $tax, 'lines' => count($b['items']), 'priority' => $priority]);
}
if ($method === 'GET' && preg_match('#^/users/([^/]+)$#', $path, $m)) {
  $id = parse_id($m[1]);
  $st = db()->prepare('select id, name, email from users where id = :id'); $st->execute(['id' => $id]);
  $row = $st->fetch(PDO::FETCH_ASSOC);
  if (!$row) err(404, 'not_found', 'user_not_found');
  reply(200, user_row($row));
}
if ($method === 'POST' && $path === '/users') {
  $b = json_body();
  if (!is_string($b['name'] ?? null) || $b['name'] === '' || mb_strlen($b['name']) > 200) invalid('body', '/name', 'length');
  if (!is_string($b['email'] ?? null) || !filter_var($b['email'], FILTER_VALIDATE_EMAIL)) invalid('body', '/email', 'Invalid email address');
  try {
    $st = db()->prepare('insert into users (name, email) values (:name, :email) returning id, name, email'); $st->execute(['name' => $b['name'], 'email' => $b['email']]);
    reply(201, user_row($st->fetch(PDO::FETCH_ASSOC)));
  } catch (PDOException $e) { if ($e->getCode() === '23505') err(409, 'conflict', 'email_taken'); throw $e; }
}
if ($method === 'POST' && preg_match('#^/orders/([^/]+)/pay$#', $path, $m)) {
  $id = parse_id($m[1]);
  $pdo = db(); $pdo->beginTransaction();
  try {
    $st = $pdo->prepare('select id, total_cents, paid from orders where id = :id for update'); $st->execute(['id' => $id]);
    $o = $st->fetch(PDO::FETCH_ASSOC);
    if (!$o) { $pdo->rollBack(); err(404, 'not_found', 'order_not_found'); }
    if ($o['paid']) { $pdo->rollBack(); err(409, 'conflict', 'already_paid'); }
    $pdo->prepare('update orders set paid = true where id = :id')->execute(['id' => $o['id']]);
    $st = $pdo->prepare('insert into payments (order_id, amount_cents) values (:o, :a) returning id'); $st->execute(['o' => $o['id'], 'a' => $o['total_cents']]);
    $pid = (int) $st->fetchColumn();
    $pdo->commit();
    reply(200, ['orderId' => (int) $o['id'], 'paymentId' => $pid, 'amountCents' => (int) $o['total_cents'], 'paid' => true]);
  } catch (Throwable $e) { if ($pdo->inTransaction()) $pdo->rollBack(); throw $e; }
}
if ($method === 'GET' && $path === '/me') {
  $key = $_SERVER['HTTP_X_API_KEY'] ?? null;
  if ($key === null || $key === '') err(401, 'unauthorized', 'missing x-api-key header');
  $st = db()->prepare('select user_id from api_keys where key = :k'); $st->execute(['k' => $key]);
  $uid = $st->fetchColumn();
  if ($uid === false) err(401, 'unauthorized', 'unknown_key');
  $st = db()->prepare('select id, name, email from users where id = :id'); $st->execute(['id' => (int) $uid]);
  $row = $st->fetch(PDO::FETCH_ASSOC);
  if (!$row) err(404, 'not_found', 'user_not_found');
  reply(200, user_row($row));
}
if ($method === 'GET' && $path === '/counter') {
  // PHP-FPM: request-scoped process state, so this is 1 unless apcu is used —
  // the honest answer for this stack is "per worker, reset per request".
  reply(200, ['count' => 1]);
}
if ($method === 'GET' && $path === '/slow') {
  $ms = $_GET['ms'] ?? '1000';
  if (!preg_match('/^[0-9]+$/', $ms) || (int) $ms > 30000) invalid('query', '/ms', 'range');
  usleep(((int) $ms) * 1000);
  reply(200, ['slept' => (int) $ms]);
}
err(404, 'route_not_found', "no route matches $method $path");

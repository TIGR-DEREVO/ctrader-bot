# cTrader Bot — Rust скелет

Минимальный проект для подключения к cTrader Open API.

## Быстрый старт

### 1. Установи Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Скачай .proto файлы от cTrader
```bash
mkdir proto
cd proto
# Скачиваем официальные proto файлы Spotware
curl -O https://raw.githubusercontent.com/spotware/openapi-proto-messages/master/OpenApiCommonMessages.proto
curl -O https://raw.githubusercontent.com/spotware/openapi-proto-messages/master/OpenApiMessages.proto
curl -O https://raw.githubusercontent.com/spotware/openapi-proto-messages/master/OpenApiModelMessages.proto
cd ..
```

### 3. Создай директорию для сгенерированного кода
```bash
mkdir src/proto
```

### 4. Настрой credentials
```bash
cp .env.example .env
# Заполни .env своими данными:
# - clientId / clientSecret → https://open.ctrader.com → My Apps
# - accessToken → OAuth flow (см. документацию cTrader)
# - accountId → номер твоего demo/live аккаунта
```

### 5. Собери и запусти
```bash
cargo build          # первый раз: скачает зависимости + скомпилирует proto
cargo run            # запуск
RUST_LOG=debug cargo run   # с детальными логами
```

## Структура проекта

```
src/
├── main.rs         # точка входа, загрузка конфига
├── connection.rs   # TCP/TLS коннектор (чтение/запись Protobuf фреймов)
├── auth.rs         # двухэтапная авторизация
└── strategy.rs     # торговая логика (сюда добавляй свой код)
```

## Как получить Access Token

cTrader использует OAuth 2.0. Упрощённый flow для разработки:

1. Зайди на https://connect.spotware.com/apps
2. Выбери своё приложение → "Playground"
3. Пройди авторизацию → скопируй `access_token`

Для продакшена — реализуй полный OAuth flow с refresh token.

## Следующие шаги

- [ ] Реализовать `handle_message()` с диспетчеризацией по `payload_type`
- [ ] Подписаться на котировки (`ProtoOASubscribeSpotsReq`)
- [ ] Добавить отправку ордеров (`ProtoOANewOrderReq`)
- [ ] Написать логику стратегии в `on_tick()`
- [ ] Добавить Prometheus метрики (`/metrics` endpoint)
- [ ] Настроить Grafana dashboard

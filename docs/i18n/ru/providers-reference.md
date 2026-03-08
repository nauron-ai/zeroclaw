# Справочник провайдеров (Русский)

Это первичная локализация Wave 1 для проверки provider ID, алиасов и переменных окружения.

Оригинал на английском:

- [../../providers-reference.md](../../providers-reference.md)

## Когда использовать

- Выбор провайдера и модели
- Проверка ID/alias/credential env vars
- Диагностика ошибок аутентификации и конфигурации

## Правило

- Provider ID и имена env переменных не переводятся.
- Нормативное описание поведения — в английском оригинале.

## Обновления

- 2026-03-01: добавлена поддержка провайдера StepFun (`stepfun`, алиасы `step`, `step-ai`, `step_ai`).
- 2026-03-08: добавлен провайдер Inception Labs (`inception`, алиас `inceptionlabs`).

## StepFun (Кратко)

- Provider ID: `stepfun`
- Алиасы: `step`, `step-ai`, `step_ai`
- Base API URL: `https://api.stepfun.com/v1`
- Эндпоинты: `POST /v1/chat/completions`, `GET /v1/models`
- Переменная авторизации: `STEP_API_KEY` (fallback: `STEPFUN_API_KEY`)
- Модель по умолчанию: `step-3.5-flash`

Быстрая проверка:

```bash
export STEP_API_KEY="your-stepfun-api-key"
zeroclaw models refresh --provider stepfun
zeroclaw agent --provider stepfun --model step-3.5-flash -m "ping"
```

## Inception Labs (Кратко)

- Provider ID: `inception`
- Алиас: `inceptionlabs`
- Base API URL: `https://api.inceptionlabs.ai/v1`
- Эндпоинты: `POST /v1/chat/completions`, `GET /v1/models`
- Переменная авторизации: `INCEPTION_API_KEY`
- Fallback через `ZEROCLAW_API_KEY` / `API_KEY` отключен
- Модель по умолчанию: `mercury-2`

Быстрая проверка:

```bash
export INCEPTION_API_KEY="your-inception-api-key"
zeroclaw models refresh --provider inception
zeroclaw agent --provider inception --model mercury-2 -m "ping"
```

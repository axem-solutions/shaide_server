FROM python:3.12-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends openssl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY mock.py entrypoint.sh ./
ENTRYPOINT ["sh", "/app/entrypoint.sh"]

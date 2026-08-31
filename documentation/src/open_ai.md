# Open AI

We implement the OpenAI API specification. The two main endpoints that the frontend relies on is the `/chat/comletions` and the `/chat` endpoints.

## Completion API
The specification can be found [here](https://platform.openai.com/docs/api-reference/chat/create).

Example usage:

```sh
curl 'http://localhost:8080/v1/chat/completions' \
  -H 'Authorization: Bearer admin' \
  -H 'Accept: application/graphql-response+json, application/graphql+json, application/json, text/event-stream, multipart/mixed' \
  -H 'Accept-Encoding: gzip, deflate, br, zstd' \
  -H 'Accept-Language: en-US' \
  -H 'Connection: keep-alive' \
  -H 'Host: localhost:8080' \
  -H 'Content-type: application/json' \
  --data-raw '
    {
      "messages": [
        {
          "role": "system",
          "content": "You are a helpful assistant."
        },
        {
          "role": "user",
          "content": "Hello! Can you tell me about sperm whales?"
        }
      ],
      "model": "openai/gpt-oss-120b-maas",
      "frequency_penalty": 0,
      "logit_bias": null,
      "logprobs": false,
      "top_logprobs": null,
      "max_tokens": 128,
      "n": 1,
      "presence_penalty": 0,
      "response_format": {
        "type": "json_object"
      },
      "seed": null,
      "service_tier": "auto",
      "stop": null,
      "stream": true,
      "stream_options": null,
      "temperature": 0.7,
      "top_p": 1,
      "tools": [],
      "tool_choice": null,
      "parallel_tool_calls": false,
      "user": "example_user_123",
      "function_call": null,
      "functions": null
    }
  '
```

## Chat API

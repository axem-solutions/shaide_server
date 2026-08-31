# Command-Line Help for `shaide-cli`

This document contains the help content for the `shaide-cli` command-line program.

**Command Overview:**

* [`shaide-cli`↴](#shaide-cli)
* [`shaide-cli add-users`↴](#shaide-cli-add-users)
* [`shaide-cli add-user`↴](#shaide-cli-add-user)
* [`shaide-cli list-users`↴](#shaide-cli-list-users)
* [`shaide-cli list-models`↴](#shaide-cli-list-models)
* [`shaide-cli list-embedding-models`↴](#shaide-cli-list-embedding-models)
* [`shaide-cli delete-model`↴](#shaide-cli-delete-model)
* [`shaide-cli delete-embedding-model`↴](#shaide-cli-delete-embedding-model)
* [`shaide-cli create-model`↴](#shaide-cli-create-model)
* [`shaide-cli create-embedding-model`↴](#shaide-cli-create-embedding-model)
* [`shaide-cli set-model-daily-limit`↴](#shaide-cli-set-model-daily-limit)
* [`shaide-cli generate-statistics`↴](#shaide-cli-generate-statistics)

## `shaide-cli`

Manage users in the database

**Usage:** `shaide-cli <COMMAND>`

###### **Subcommands:**

* `add-users` — Add new users to the database
* `add-user` — Add a single user to the database with the provided username
* `list-users` — List all users in the database
* `list-models` — List the models in the DB
* `list-embedding-models` — List embedding models
* `delete-model` — Deletes a model based on the ID
* `delete-embedding-model` — Delete an embedding model based on the ID
* `create-model` — Create a model
* `create-embedding-model` — Create an embedding model
* `set-model-daily-limit` — Set a model daily limit
* `generate-statistics` — Generates statistics



## `shaide-cli add-users`

Add new users to the database

**Usage:** `shaide-cli add-users --remote <REMOTE> --admin-password <ADMIN_PASSWORD> <NUMBER_OF_USERS>`

###### **Arguments:**

* `<NUMBER_OF_USERS>` — The number of users to add

###### **Options:**

* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli add-user`

Add a single user to the database with the provided username

**Usage:** `shaide-cli add-user --expiry <EXPIRY> --remote <REMOTE> --admin-password <ADMIN_PASSWORD> <USERNAME> <PASSWORD>`

###### **Arguments:**

* `<USERNAME>`
* `<PASSWORD>`

###### **Options:**

* `--expiry <EXPIRY>`
* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli list-users`

List all users in the database

**Usage:** `shaide-cli list-users --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli list-models`

List the models in the DB

**Usage:** `shaide-cli list-models --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli list-embedding-models`

List embedding models

**Usage:** `shaide-cli list-embedding-models --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli delete-model`

Deletes a model based on the ID

**Usage:** `shaide-cli delete-model --id <ID> --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--id <ID>`
* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli delete-embedding-model`

Delete an embedding model based on the ID

**Usage:** `shaide-cli delete-embedding-model --id <ID> --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--id <ID>`
* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli create-model`

Create a model

**Usage:** `shaide-cli create-model [OPTIONS] --name <NAME> --variant <VARIANT> --chat-completions-endpoint <CHAT_COMPLETIONS_ENDPOINT> --api-schema <API_SCHEMA> --max-generated-tokens <MAX_GENERATED_TOKENS> --context-size <CONTEXT_SIZE> --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--name <NAME>`
* `--variant <VARIANT>`
* `--chat-completions-endpoint <CHAT_COMPLETIONS_ENDPOINT>`
* `--completions-endpoint <COMPLETIONS_ENDPOINT>`
* `--responses-endpoint <RESPONSES_ENDPOINT>`
* `--api-schema <API_SCHEMA>`
* `--native-fim-mode <NATIVE_FIM_MODE>`
* `--fim-prompt-template <FIM_PROMPT_TEMPLATE>`
* `--daily-input-token-limit <DAILY_INPUT_TOKEN_LIMIT>`
* `--daily-output-token-limit <DAILY_OUTPUT_TOKEN_LIMIT>`
* `--supports-images <SUPPORTS_IMAGES>`

  Possible values: `true`, `false`

* `--reasoning-effort-values <REASONING_EFFORT_VALUES>` — Values the model accepts for `reasoning_effort`, in the order clients should render them (for example `minimal,low,medium,high`). Leave it out for models that do not accept the parameter, including reasoning models driven by a thinking mode or a token budget
* `--max-images-per-request <MAX_IMAGES_PER_REQUEST>`
* `--max-image-bytes <MAX_IMAGE_BYTES>`
* `--max-image-width-px <MAX_IMAGE_WIDTH_PX>`
* `--max-image-height-px <MAX_IMAGE_HEIGHT_PX>`
* `--max-generated-tokens <MAX_GENERATED_TOKENS>`
* `--context-size <CONTEXT_SIZE>`
* `--platform <PLATFORM>`
* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli create-embedding-model`

Create an embedding model

**Usage:** `shaide-cli create-embedding-model [OPTIONS] --url <URL> --name <NAME> --vector-size <VECTOR_SIZE> --platform <PLATFORM> --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--url <URL>`
* `--name <NAME>`
* `--vector-size <VECTOR_SIZE>`
* `--max-embedding-model-text-len <MAX_EMBEDDING_MODEL_TEXT_LEN>`
* `--platform <PLATFORM>`
* `--api-schema <API_SCHEMA>`
* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli set-model-daily-limit`

Set a model daily limit

**Usage:** `shaide-cli set-model-daily-limit [OPTIONS] --model-name <MODEL_NAME> --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--model-name <MODEL_NAME>`
* `-i`, `--input <DAILY_INPUT_TOKEN_LIMIT>`
* `-o`, `--output <DAILY_OUTPUT_TOKEN_LIMIT>`
* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



## `shaide-cli generate-statistics`

Generates statistics

**Usage:** `shaide-cli generate-statistics --start-date <START_DATE> --end-date <END_DATE> --remote <REMOTE> --admin-password <ADMIN_PASSWORD>`

###### **Options:**

* `--start-date <START_DATE>`
* `--end-date <END_DATE>`
* `--remote <REMOTE>`
* `--admin-password <ADMIN_PASSWORD>` — Password for the built-in admin user



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

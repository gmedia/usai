[@sakaladev/usai](../../README.md) / [index](../README.md) / MultipartBody

# Interface: MultipartBody

A parsed `multipart/form-data` body.

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="fields"></a> `fields` | `Record`\<`string`, `string`\> | Text fields (a repeated name keeps the last value; use `all` for every value). |
| <a id="files"></a> `files` | [`MultipartFile`](MultipartFile.md)[] | - |
| <a id="all"></a> `all` | \{ `name`: `string`; `value`: `string`; \}[] | Every field value in order, repeated names included. |

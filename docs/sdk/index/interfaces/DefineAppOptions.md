[@sakaladev/usai](../../README.md) / [index](../README.md) / DefineAppOptions

# Interface: DefineAppOptions

Options for [defineApp](../functions/defineApp.md).

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | Application name: the OpenAPI title and the reference page's heading. |
| <a id="description"></a> `description?` | `string` | One paragraph about the application, shown on the reference page's overview and as `info.description` of the generated OpenAPI document. Plain text (no markup). Not part of the application identity. |
| <a id="headers"></a> `headers?` | `Record`\<`string`, `string`\> | Response headers set on every response of the application's routes (not on `/_usai/*`): the security headers a proxy would otherwise add (`strict-transport-security`, `x-content-type-options`, `content-security-policy` …). A handler's own header of the same name wins. Static values only — anything computed belongs in the handler. |
| <a id="modules"></a> `modules?` | [`ModuleDeclaration`](ModuleDeclaration.md)[] | - |
| <a id="workloads"></a> `workloads?` | [`Workload`](Workload.md)[] | - |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | - |
| <a id="env"></a> `env?` | [`EnvDeclaration`](EnvDeclaration.md)\<`Record`\<`string`, [`EnvField`](EnvField.md)\<`unknown`\>\>\> | - |

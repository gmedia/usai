[@sakaladev/usai](../../README.md) / [index](../README.md) / DefineAppOptions

# Interface: DefineAppOptions

Options for [defineApp](../functions/defineApp.md).

## Properties

| Property | Type | Description |
| ------ | ------ | ------ |
| <a id="name"></a> `name?` | `string` | Application name: the OpenAPI title and the reference page's heading. |
| <a id="description"></a> `description?` | `string` | One paragraph about the application, shown on the reference page's overview and as `info.description` of the generated OpenAPI document. Plain text (no markup). Not part of the application identity. |
| <a id="headers"></a> `headers?` | `Record`\<`string`, `string`\> | Response headers set on every response of the application's routes (not on `/_usai/*`): the security headers a proxy would otherwise add (`strict-transport-security`, `x-content-type-options`, `content-security-policy` …). A handler's own header of the same name wins. Static values only — anything computed belongs in the handler. |
| <a id="modules"></a> `modules?` | [`ModuleDeclaration`](ModuleDeclaration.md)[] | The modules this application is composed of (`defineModule`). A module brings its own workloads, resources, migrations and seeders, so a larger application lists modules here and workloads nowhere. |
| <a id="workloads"></a> `workloads?` | [`Workload`](Workload.md)[] | The workloads that do not belong to a module — routes, tasks, cron entries, consumers, services, commands. **A workload exists because this list (or a module's) reaches it through an `import`: the runtime never scans your files**, so a route file nobody imports is not served. |
| <a id="resources"></a> `resources?` | [`ResourceDeclaration`](ResourceDeclaration.md)\<`string`, `unknown`\>[] | Resources the application opens that no workload declares — rare: a resource named in a workload's `resources: [...]` is already part of the application. Everything here is opened at activation. |
| <a id="env"></a> `env?` | [`EnvDeclaration`](EnvDeclaration.md)\<`Record`\<`string`, [`EnvField`](EnvField.md)\<`unknown`\>\>\> | The environment the whole application requires (`env({ … })`): every variable, its type and whether it is required. A missing required variable fails activation with all of them named at once, never at the first request. |

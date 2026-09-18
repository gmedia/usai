// Standard Schema interop (ADR-0002). The types below are the published
// `@standard-schema/spec` v1 surface, inlined so the SDK has no runtime
// dependency on any schema library.

export interface StandardSchemaV1<Input = unknown, Output = Input> {
  readonly "~standard": StandardSchemaV1.Props<Input, Output>;
}

export declare namespace StandardSchemaV1 {
  export interface Props<Input = unknown, Output = Input> {
    readonly version: 1;
    readonly vendor: string;
    readonly validate: (value: unknown) => Result<Output> | Promise<Result<Output>>;
    readonly types?: Types<Input, Output> | undefined;
    /** Standard JSON Schema v1: present when the vendor can describe itself. */
    readonly jsonSchema?: JsonSchemaProps | undefined;
  }
  export type Result<Output> = SuccessResult<Output> | FailureResult;
  export interface SuccessResult<Output> {
    readonly value: Output;
    readonly issues?: undefined;
  }
  export interface FailureResult {
    readonly issues: ReadonlyArray<Issue>;
  }
  export interface Issue {
    readonly message: string;
    readonly path?: ReadonlyArray<PropertyKey | PathSegment> | undefined;
  }
  export interface PathSegment {
    readonly key: PropertyKey;
  }
  export interface Types<Input = unknown, Output = Input> {
    readonly input: Input;
    readonly output: Output;
  }
  export type InferInput<S extends StandardSchemaV1> = NonNullable<S["~standard"]["types"]>["input"];
  export type InferOutput<S extends StandardSchemaV1> = NonNullable<S["~standard"]["types"]>["output"];
  export interface JsonSchemaProps {
    readonly input: (options: JsonSchemaOptions) => Record<string, unknown>;
    readonly output: (options: JsonSchemaOptions) => Record<string, unknown>;
  }
  export interface JsonSchemaOptions {
    readonly target: "draft-2020-12" | "draft-07" | "openapi-3.0" | (string & {});
    readonly libraryOptions?: Record<string, unknown> | undefined;
  }
}

export type AnySchema = StandardSchemaV1;
export type Output<S> = S extends StandardSchemaV1 ? StandardSchemaV1.InferOutput<S> : never;

export function isSchema(value: unknown): value is StandardSchemaV1 {
  return typeof value === "object" && value !== null && "~standard" in value;
}

export interface ValidationIssue {
  message: string;
  /** JSON pointer (`/tags/0`), the same format the host uses for
   * boundary validation, so clients see one shape wherever the check ran. */
  path: string;
}

function pointer(segments: readonly unknown[]): string {
  return segments
    .map((segment) => {
      const key = typeof segment === "object" && segment !== null && "key" in segment ? (segment as { key: unknown }).key : segment;
      return "/" + String(key).replaceAll("~", "~0").replaceAll("/", "~1");
    })
    .join("");
}

export type Validation<T> = { ok: true; value: T } | { ok: false; issues: ValidationIssue[] };

/** Runs the provider's validator. Async providers are not supported inside a
 * world in v0; they surface as a single issue rather than a hang. */
export function validateWith<S extends StandardSchemaV1>(schema: S, value: unknown): Validation<Output<S>> {
  const result = schema["~standard"].validate(value);
  if (result instanceof Promise) {
    return { ok: false, issues: [{ message: "asynchronous schema validation is not supported", path: "" }] };
  }
  if (result.issues) {
    return {
      ok: false,
      issues: result.issues.map((issue) => ({
        message: issue.message,
        path: pointer(issue.path ?? []),
      })),
    };
  }
  return { ok: true, value: result.value as Output<S> };
}

/** JSON Schema for the schema's *input* side, or `undefined` when the
 * provider cannot describe itself. Never throws: a provider that fails to
 * describe one schema degrades that contract to in-world validation. */
export function jsonSchemaOf(schema: StandardSchemaV1, side: "input" | "output" = "input"): Record<string, unknown> | undefined {
  const describe = schema["~standard"].jsonSchema;
  if (!describe) return undefined;
  try {
    const out = describe[side]({ target: "draft-2020-12" });
    return typeof out === "object" && out !== null ? out : undefined;
  } catch {
    return undefined;
  }
}

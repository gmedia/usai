// Authentication is a declared boundary (ADR-0004), reused by reference.

import type { AuthDeclaration } from "./declarations.ts";
import type { BaseContext } from "./runtime/context.ts";

export interface AuthRequest {
  readonly method: string;
  readonly path: string;
  readonly headers: Record<string, string>;
  readonly query: Record<string, string | string[]>;
}

export interface BearerOptions<P> {
  name?: string;
  /** Return the principal, or throw `errors.unauthorized()`. */
  resolve: (ctx: BaseContext & { request: AuthRequest }, token: string) => P | Promise<P>;
}

export interface HeaderOptions<P> {
  name?: string;
  header: string;
  resolve: (ctx: BaseContext & { request: AuthRequest }, value: string) => P | Promise<P>;
}

export interface CustomOptions<P> {
  name: string;
  resolve: (ctx: BaseContext & { request: AuthRequest }) => P | Promise<P>;
}

let anonymous = 0;

export const auth = {
  bearer<P>(options: BearerOptions<P>): AuthDeclaration<P> {
    return {
      __usai: "auth",
      name: options.name ?? `bearer-${++anonymous}`,
      scheme: "bearer",
      header: "authorization",
      resolve: options.resolve as AuthDeclaration<P>["resolve"],
    };
  },
  header<P>(options: HeaderOptions<P>): AuthDeclaration<P> {
    return {
      __usai: "auth",
      name: options.name ?? `header-${++anonymous}`,
      scheme: "header",
      header: options.header.toLowerCase(),
      resolve: options.resolve as AuthDeclaration<P>["resolve"],
    };
  },
  custom<P>(options: CustomOptions<P>): AuthDeclaration<P> {
    return {
      __usai: "auth",
      name: options.name,
      scheme: "custom",
      resolve: ((ctx: BaseContext & { request: AuthRequest }) => options.resolve(ctx)) as AuthDeclaration<P>["resolve"],
    };
  },
};

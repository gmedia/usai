// Password hashing is a host operation: Argon2id is deliberately expensive
// and must not run on a world's thread. The hash is a PHC string
// (`$argon2id$v=19$m=19456,t=2,p=1$…`) you store as text.

import { op } from "./runtime/context.ts";

export const password = {
  /** Argon2id, random 16-byte salt, the runtime's default cost. */
  hash(plain: string): Promise<string> {
    return op<string>("crypto", { op: "password-hash", password: plain });
  },
  /** Constant-time verification against a PHC hash; `false`, never a throw, for a wrong password. */
  verify(plain: string, hash: string): Promise<boolean> {
    return op<boolean>("crypto", { op: "password-verify", password: plain, hash });
  },
};

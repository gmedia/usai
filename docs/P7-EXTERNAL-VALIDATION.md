# P7 — external developer validation: the kit

The last gate before contracts freeze is not a test the maintainers can run:
**three to ten developers who have never seen Usai build and deploy a small
API from the public documentation alone**, and say where it fought them
(`docs/ROADMAP.md` P7). Six internal fresh-eyes rounds (subagents playing
personas) were the rehearsal and fixed what they found; they are not the
evidence, because they knew too much.

This page is what a participant receives. It is deliberately short: the
point is what *they* do with the docs, not what we tell them.

## For the participant

Thank you. You are about to spend about **90 minutes** building a small API
on a runtime you have not used. We are not testing you; we are testing the
documentation and the runtime. Everything that confuses you is a finding,
and the more of them you write down the more useful the session is.

**Before the timer starts** (this part is setup, not the session — do it
first and do not count it):

- **Node 24 or newer** and a package manager (`pnpm` in the examples;
  `npm`/`yarn` work). `node --version` should say `v24` or higher.
- **A PostgreSQL you can throw away.** Any of these:
  ```bash
  docker run -d --name p7-pg -p 5432:5432 \
    -e POSTGRES_USER=usai -e POSTGRES_PASSWORD=usai -e POSTGRES_DB=usai postgres:18
  # → DATABASE_URL=postgres://usai:usai@127.0.0.1:5432/usai
  ```
  a managed database, or one you already run. You never need superuser
  rights; the application creates its own tables.
- **Nothing else.** The runtime binary is fetched for you on the first
  command (`~/.cache/usai/<version>/usai`); you do not install it, and you
  do not need Rust, Docker (beyond the database above) or a cluster.
- Write down `pnpm exec usai --version` — the report needs it.

**Rules**

- Use only what is public: the README, `docs/GUIDE.md`, `docs/sdk/`, the
  `/_usai/docs` page of your running application, and the runtime's own
  messages. Do not read the runtime's source. If you had to, that is a
  finding — write down what you were looking for.
- Do not ask a maintainer during the session. Write the question down with
  the time; we answer afterwards.
- Keep a timer running from the moment you start step 1.

**The task** — a tiny "reading list" API:

1. Scaffold a project (`pnpm dlx @sakaladev/create-usai reading-list`), start
   it, get a `200` from the hello route. *Record the time.*
2. Add a PostgreSQL table `books (id, title, author, read boolean)` through a
   migration, and two routes: `POST /books` (validated body: title and
   author, both required, title at most 200 characters) and `GET /books`
   (paginated, `?limit=&offset=`, at most 50 per page). *Record the time of
   the first `201` and the first `200` with rows.*
3. Add a task `notifyAdded` that receives the new book's id and logs a line;
   `POST /books` must hand the work to it without waiting for it.
4. Protect `POST /books` with an API key (a header of your choice) checked
   against a fixed value from the environment; `GET /books` stays public.
5. Write one test with `usai/test` that creates a book and lists it.
6. Build the artifact and run it with the production command (`usai build`,
   then `usai run --artifact …`), and check `/_usai/ready` answers `200`.
   *Record the time.*
7. Make one deliberate mistake of your choice (a wrong type, a missing
   environment variable, a forgotten `await`, an undeclared resource) and
   note what the runtime told you and whether it helped.

Stop at 90 minutes whether or not you finished; an unfinished step is a
finding about the step.

**What to send back** — file it as an issue with the "P7 report" template,
or send the same content by mail:

- Your background (which stacks you use daily), your OS, Node version.
- The three timestamps: first `200`, first row from PostgreSQL, first
  production `200`.
- For each step: done / not done, and every moment you hesitated — what you
  expected, what you found, what you did instead.
- Which documentation page you opened most, and which one you wished
  existed.
- Any error message that did not tell you what to do.
- Whether the lifecycle model (a fresh execution world per request, `invoke`
  vs `dispatch`, resources declared per workload) felt natural or in the way,
  in one paragraph.
- One thing you would change first.

## For the maintainers

- Recruit developers who match the people Usai is for: someone shipping
  APIs today on Express/Fastify/Hono, Laravel/Symfony, Rails, Django/FastAPI,
  Go or Spring. Mix them; five people making the same mistake means the API
  is wrong, one person means the doc is.
- Give them the current release, not `main`; note the version on the report.
  Today that is **0.0.9** (`pnpm dlx @sakaladev/create-usai@latest` scaffolds
  a project pinned to it). Hand them this page and nothing else — no demo, no
  walkthrough, no "one thing to know first". The first five minutes of
  someone alone with the documentation is the most expensive data this
  project can buy, and a hint spends it.
- Do not help during the session. Afterwards, walk the report with them for
  fifteen minutes and write down the questions they ask *then* too.
- Every report becomes a row in `docs/STATUS.md` (P7) with what was fixed;
  the timestamps go into the same table so the trend across participants is
  visible. The roadmap's P7 exit is: the timestamps stop improving between
  fixes, and no participant needed the source.

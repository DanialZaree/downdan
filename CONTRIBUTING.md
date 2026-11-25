# Contributing to danDown 🦀 ⬇️

First of all — thank you for taking the time to contribute to **danDown**! 🌹  
danDown is a blazing-fast, crash-resistant, and extensible download manager built with **Rust** + **Tauri** + **Next.js**.  
We welcome all kinds of contributions — from fixing typos to building new features.

---

## 📜 Code of Conduct

Please read our [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before participating.  
We are committed to providing a welcoming and harassment-free experience for everyone.

---

## 🚀 How Can I Contribute?

### 1. Reporting Bugs

- Search existing issues to avoid duplicates.
- Use the **"Bug report"** template.
- Include:
  - OS & version (Windows, macOS, Linux)
  - danDown version
  - Steps to reproduce
  - Expected vs actual behavior
  - Logs/screenshots if possible

### 2. Suggesting Features

- Use the **"Feature request"** template.
- Explain the problem your idea solves, not just the solution.
- Describe a possible UI/UX approach if relevant.

### 3. Improving Documentation

- Fix typos, improve clarity, or add missing info.
- Update any **README.md** or **docs/** content.
- Ensure Markdown formatting is clean.

---

## 🛠 Development Setup

### Prerequisites

- **Rust** (latest stable) → [Install Rust](https://www.rust-lang.org/tools/install)
- **Node.js** (LTS) → [Install Node.js](https://nodejs.org/)
- **pnpm** package manager → `npm install -g pnpm`
- **SQLite** (for local database)

### 1. Clone & Install

```bash
git clone https://github.com/mehranTaslimi/danDown.git
cd danDown
pnpm install
```

### 2. Create .env file in the project root

```
DATABASE_URL=sqlite://./database.db?mode=rwc
RUST_LOG=dandown_lib=debug
```

### 3. Run database migrations

```bash
# Install sqlx-cli (for database migrations)
cargo install sqlx-cli --no-default-features --features sqlite

cd src-tauri
cargo sqlx migrate run
```

### 4. Run in Development

```bash
pnpm dev:all
```

### Build for Production

```bash
pnpm build
```

---

## 🧹 Code Style & Standards

danDown follows **modern, clean, and idiomatic code practices**.

### Rust

- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).
- Run `cargo fmt` before committing.
- Run `cargo clippy --all-targets --all-features` and fix warnings.
- Add tests for new features when possible.

### Frontend (Next.js + shadcn/ui)

- Follow [shadcn/ui guidelines](https://ui.shadcn.com/).
- Use TailwindCSS utilities where possible.
- Keep components small and reusable.

### Commits

We use **[Conventional Commits](https://www.conventionalcommits.org/)**:

```
feat: add torrent magnet link support
fix: prevent crash when proxy auth fails
chore: update dependencies
docs: improve README setup instructions
```

---

## 🌿 Branch & PR Workflow

1. **Create a branch** from `main`:
   ```bash
   git checkout -b feature/my-feature
   ```
2. **Make changes**, commit with Conventional Commits.
3. **Push** your branch:
   ```bash
   git push origin feature/my-feature
   ```
4. **Open a Pull Request**:
   - Link any related issues (`Fixes #123`).
   - Describe what and why you changed.
   - Add screenshots/GIFs for UI changes.

✅ **PR checks** must pass before merge:

- Lint passes (`pnpm lint` + `cargo clippy`)
- Builds successfully (`pnpm build`)

---

## 📄 License

By contributing, you agree that your contributions will be licensed under the same license as danDown — [MIT OR Apache-2.0](LICENSE).

---

**Thank you for making danDown better!** ❤️  
Your contributions power this project.

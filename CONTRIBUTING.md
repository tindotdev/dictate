<!-- omit in toc -->

# Contributing to dictate

First off, thanks for taking the time to contribute! ❤️

All types of contributions are encouraged and valued. See the [Table of Contents](#table-of-contents) for different ways to help and details about how this project handles them. Please make sure to read the relevant section before making your contribution. It will make it a lot easier for us maintainers and smooth out the experience for all involved. The community looks forward to your contributions. 🎉

> **Important:** Please read our [Code of Conduct](CODE_OF_CONDUCT.md) before participating.

> And if you like the project, but just don't have time to contribute, that's fine. There are other easy ways to support the project and show your appreciation, which we would also be very happy about:
>
> - Star the project
> - Tweet about it
> - Refer this project in your project's readme
> - Mention the project at local meetups and tell your friends/colleagues

<!-- omit in toc -->

## Table of Contents

- [I Have a Question](#i-have-a-question)
  - [I Want To Contribute](#i-want-to-contribute)
  - [Reporting Bugs](#reporting-bugs)
  - [Suggesting Enhancements](#suggesting-enhancements)
  - [Your First Code Contribution](#your-first-code-contribution)
  - [Improving The Documentation](#improving-the-documentation)
- [Styleguides](#styleguides)
  - [Commit Messages](#commit-messages)

## I Have a Question

> If you want to ask a question, we assume that you have read the available [README](README.md) and tried `:checkhealth dictate` in Neovim for troubleshooting.

Before you ask a question, it is best to search for existing [Issues](https://github.com/tindotdev/dictate/issues) that might help you. In case you have found a suitable issue and still need clarification, you can write your question in this issue. It is also advisable to search the internet for answers first.

If you then still feel the need to ask a question and need clarification, we recommend the following:

- Open an [Issue](https://github.com/tindotdev/dictate/issues/new).
- Provide as much context as you can about what you're running into.
- Provide project and platform versions (Bun, Neovim, OS, etc), depending on what seems relevant.

We will then take care of the issue as soon as possible.

<!--
You might want to create a separate issue tag for questions and include it in this description. People should then tag their issues accordingly.

Depending on how large the project is, you may want to outsource the questioning, e.g. to Stack Overflow or Gitter. You may add additional contact and information possibilities:
- IRC
- Slack
- Gitter
- Stack Overflow tag
- Blog
- FAQ
- Roadmap
- E-Mail List
- Forum
-->

## I Want To Contribute

> ### Legal Notice <!-- omit in toc -->
>
> When contributing to this project, you must agree that you have authored 100% of the content, that you have the necessary rights to the content and that the content you contribute may be provided under the project licence.

### Reporting Bugs

<!-- omit in toc -->

#### Before Submitting a Bug Report

A good bug report shouldn't leave others needing to chase you up for more information. Therefore, we ask you to investigate carefully, collect information and describe the issue in detail in your report. Please complete the following steps in advance to help us fix any potential bug as fast as possible.

- Make sure that you are using the latest version.
- Determine if your bug is really a bug and not an error on your side e.g. using incompatible environment components/versions (Make sure that you have read the [README](README.md). If you are looking for support, you might want to check [this section](#i-have-a-question)).
- To see if other users have experienced (and potentially already solved) the same issue you are having, check if there is not already a bug report existing for your bug or error in the [bug tracker](https://github.com/tindotdev/dictate/issues?q=label%3Abug).
- Also make sure to search the internet (including Stack Overflow) to see if users outside of the GitHub community have discussed the issue.
- Collect information about the bug:
  - Stack trace (Traceback)
  - OS, Platform and Version (Windows, Linux, macOS, x86, ARM)
  - Version of the interpreter, compiler, SDK, runtime environment, package manager, depending on what seems relevant.
  - Possibly your input and the output
  - Can you reliably reproduce the issue? And can you also reproduce it with older versions?

<!-- omit in toc -->

#### How Do I Submit a Good Bug Report?

> You must never report security related issues, vulnerabilities or bugs including sensitive information to the issue tracker, or elsewhere in public. Instead sensitive bugs must be sent by email to <tin@tindev.dev>. See [SECURITY.md](SECURITY.md) for details.

<!-- You may add a PGP key to allow the messages to be sent encrypted as well. -->

We use GitHub issues to track bugs and errors. If you run into an issue with the project:

- Open an [Issue](https://github.com/tindotdev/dictate/issues/new). (Since we can't be sure at this point whether it is a bug or not, we ask you not to talk about a bug yet and not to label the issue.)
- Explain the behavior you would expect and the actual behavior.
- Please provide as much context as possible and describe the _reproduction steps_ that someone else can follow to recreate the issue on their own. This usually includes your code. For good bug reports you should isolate the problem and create a reduced test case.
- Provide the information you collected in the previous section.

Once it's filed:

- The project team will label the issue accordingly.
- A team member will try to reproduce the issue with your provided steps. If there are no reproduction steps or no obvious way to reproduce the issue, the team will ask you for those steps and mark the issue as `needs-repro`. Bugs with the `needs-repro` tag will not be addressed until they are reproduced.
- If the team is able to reproduce the issue, it will be marked `needs-fix`, as well as possibly other tags (such as `critical`), and the issue will be left to be [implemented by someone](#your-first-code-contribution).

<!-- You might want to create an issue template for bugs and errors that can be used as a guide and that defines the structure of the information to be included. If you do so, reference it here in the description. -->

### Suggesting Enhancements

This section guides you through submitting an enhancement suggestion for dictate, **including completely new features and minor improvements to existing functionality**. Following these guidelines will help maintainers and the community to understand your suggestion and find related suggestions.

<!-- omit in toc -->

#### Before Submitting an Enhancement

- Make sure that you are using the latest version.
- Read the [README](README.md) carefully and find out if the functionality is already covered, maybe by an individual configuration.
- Perform a [search](https://github.com/tindotdev/dictate/issues) to see if the enhancement has already been suggested. If it has, add a comment to the existing issue instead of opening a new one.
- Find out whether your idea fits with the scope and aims of the project. It's up to you to make a strong case to convince the project's developers of the merits of this feature. Keep in mind that we want features that will be useful to the majority of our users and not just a small subset. If you're just targeting a minority of users, consider writing an add-on/plugin library.

<!-- omit in toc -->

#### How Do I Submit a Good Enhancement Suggestion?

Enhancement suggestions are tracked as [GitHub issues](https://github.com/tindotdev/dictate/issues).

- Use a **clear and descriptive title** for the issue to identify the suggestion.
- Provide a **step-by-step description of the suggested enhancement** in as many details as possible.
- **Describe the current behavior** and **explain which behavior you expected to see instead** and why. At this point you can also tell which alternatives do not work for you.
- **Explain why this enhancement would be useful** to most dictate users. You may also want to point out the other projects that solved it better and which could serve as inspiration.

<!-- You might want to create an issue template for enhancement suggestions that can be used as a guide and that defines the structure of the information to be included. If you do so, reference it here in the description. -->

### Your First Code Contribution

#### Prerequisites

- [Bun](https://bun.sh/) - Required runtime
- [Cargo](https://www.rust-lang.org/tools/install) - For Lua tooling (stylua, selene)
- Git
- (Optional) Neovim - For testing the plugin

#### Setup

```bash
# Clone the repository
git clone https://github.com/tindotdev/dictate.git
cd dictate

# Install dependencies
bun install

# Install Lua formatting and linting tools
cargo install stylua selene --locked
```

#### Development Workflow

**Run all checks (linting + formatting):**

```bash
bun run check
```

This runs both:

- `bun run check:daemon` - TypeScript linting
- `bun run check:nvim` - Lua formatting and linting

**Daemon development:**

```bash
cd daemon && bun run dev      # Development mode
cd daemon && bun run build    # Build
DEBUG=1 bun daemon/src/main.ts  # Debug mode with verbose logging
```

#### Testing

**Daemon tests:**

```bash
cd daemon && bun test                # Run all tests
cd daemon && bun test --coverage     # Run with coverage report
```

**Lua tests:**

```bash
# One-time setup: Install plenary.nvim
git clone --depth 1 https://github.com/nvim-lua/plenary.nvim \
  ~/.local/share/nvim/site/pack/vendor/start/plenary.nvim

# Run Lua tests
nvim --headless -c "lua require('plenary.test_harness').test_directory('nvim/tests', {minimal_init = 'nvim/tests/minimal_init.lua'})" -c "qa!"
```

**Manual testing:**

- See `daemon/TEST_CHECKLIST.md` for manual test scenarios
- Run `:checkhealth dictate` in Neovim to verify your setup

#### Before Submitting a Pull Request

1. **Run all checks:** `bun run check` (must pass)
2. **Run all tests:** Both daemon and Lua tests must pass
3. **Test manually** if your changes affect core functionality
4. **Commit your changes** with clear, descriptive messages
5. **Push to your fork** and open a Pull Request

#### Pull Request Guidelines

- Create a feature branch from `main`
- Keep changes focused and atomic
- Write clear commit messages (see [Commit Messages](#commit-messages))
- Update documentation if needed
- Respond to review feedback promptly

### Improving The Documentation

Documentation improvements are always welcome! This includes:

- **README.md** - Installation instructions, usage examples, troubleshooting
- **docs/** - Architecture docs, runbooks, guides
- **Code comments** - Clarifying complex logic
- **CHANGELOG.md** - Keeping release notes accurate

Submit documentation PRs the same way as code PRs. Documentation changes don't require tests but should be clear and accurate.

## Styleguides

### Commit Messages

- Use present tense ("Add feature" not "Added feature")
- Use imperative mood ("Move cursor to..." not "Moves cursor to...")
- Keep the first line under 72 characters
- Reference issues and PRs when applicable (e.g., "Fix #123")
- Be descriptive but concise
- Explain _why_ not just _what_ in the commit body if needed

**Examples:**

```
fix: resolve audio capture crash on macOS Sonoma

The ffmpeg process was not handling device permissions correctly.
Added proper error handling and user feedback.

Fixes #456
```

```
feat: add support for custom OpenAI endpoints

Allows users to specify alternative API endpoints in config.
Useful for proxies and custom deployments.
```

<!-- omit in toc -->

## Attribution

This guide is based on the [contributing.md](https://contributing.md/generator)!

# GitHub Commit Notifier

A simple yet powerful tool that monitors specified GitHub organizations for new commits and sends real-time notifications to your terminal.

## Features

- **Real-Time Notifications**: Get instant alerts for new commits, both in the terminal and via desktop notifications.
- **Organization-Wide Monitoring**: Keep an eye on all repositories within one or more GitHub organizations.
- **Detailed Commit Information**: Notifications include the repository name, commit author, commit message, and a direct link to the commit.
- **Efficient and Fast**: Utilizes asynchronous requests to fetch data quickly without slowing you down.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) installed on your system.
- `notify-send` installed on your system (for desktop notifications). This is usually available by default on most Linux distributions.
- A GitHub personal access token with the `repo` scope. You can create one [here](https://github.com/settings/tokens).

### Installation

1. **Clone the repository:**

   ```bash
   git clone https://github.com/kalidyasin/github-commit-notifier.git
   cd github-commit-notifier
   ```

2. **Create a `.env` file** in the root of the project and add the following environment variables:

   ```
   GITHUB_TOKEN=your_personal_access_token
   ORGS=org1,org2
   ```

   - `GITHUB_TOKEN`: Your GitHub personal access token.
   - `ORGS`: A comma-separated list of GitHub organizations to monitor.
   - `SLEEP_SECS`: (Optional) The interval in seconds between checks for new commits. Defaults to `60` if not set.

3. **Build and run the application:**

   ```bash
   cargo run
   ```

## Notification Format

When a new commit is detected, a notification will be printed to your terminal in the following format:

```
New commit in <organization>/<repository>
By <Author Name>: <Commit Message>
URL: <Link to Commit>
```

## Technologies Used

- [Rust](https://www.rust-lang.org/)
- [Tokio](https://tokio.rs/)
- [Reqwest](https://docs.rs/reqwest/latest/reqwest/)
- [Serde](https://serde.rs/)
- [Futures](https://rust-lang.github.io/futures-rs/)

# GitLab 19.5.3 file-hook captures

GitLab Community Edition `19.5.3-ce.0` emitted these five deliveries on November 19, 2026, in a disposable self-managed installation. GitLab's Sidekiq file-hook worker invoked the unmodified companion `parasaur-forward`. A local HTTP receiver checked the shared secret and saved the request body. GitLab and the receiver shared a network namespace with only loopback enabled. [Forwarder at the captured revision](https://github.com/Kh1ng/parasaur-gitlab/blob/d77d3cac08ab2b826be5ddb22335443d1dbf0727/server/file_hooks/parasaur-forward)

| File | Action that produced the delivery |
| --- | --- |
| `issue.json` | The maintainer assigned an existing issue to `parasaur`. |
| `merge_request.json` | The maintainer requested review from `parasaur` on an open merge request. |
| `note.json` | The maintainer mentioned `@parasaur` in an issue comment. |
| `note-mr.json` | The maintainer mentioned `@parasaur` in a merge request comment. |
| `pipeline.json` | The maintainer changed the commit status for the merge request branch from running to failed. GitLab marked its push pipeline failed and emitted the hook. |

To repeat the capture, install the forwarder in GitLab's `file_hooks` directory and set its destination to a local receiver. Create two synthetic users, a project initialized with a README, an issue, and a merge request from a branch containing a CI job. Perform the actions above through the GitLab API and select the event for each transition. [GitLab file hooks](https://docs.gitlab.com/administration/file_hooks/)

The project, users, email addresses, tasks, and commit were created for the fixture. Avatar URLs containing Gravatar hashes were replaced with URLs under `gitlab.example.com`. The JSON was indented for review. The captures retain GitLab's field names, values, and structure elsewhere, including the pipeline's empty variables list.

const MARKER = "<!-- codex-lint-feedback -->";

function isSuccessful(result) {
    return result === "success" || result === "skipped";
}

function collectFindings(env) {
    const findings = [];

    if (!isSuccessful(env.LINT_RESULT)) {
        findings.push(
            "`Lint Gate (Format + Clippy + Strict Delta)` failed. Check the workflow logs for the exact formatter, clippy, or delta-lint violation."
        );
    }

    if (!isSuccessful(env.DOCS_RESULT)) {
        findings.push(
            "`Docs Quality` failed. Review markdown lint or offline link-check output in the docs-quality job."
        );
    }

    return findings;
}

function buildBody(findings) {
    const bullets = findings.map((finding) => `- ${finding}`).join("\n");
    return `${MARKER}
CI found actionable feedback on this PR:

${bullets}

This comment is updated automatically by the workflow.`;
}

async function upsertComment({ github, context, body }) {
    const issue_number = context.payload.pull_request?.number;
    if (!issue_number) {
        return;
    }

    const { owner, repo } = context.repo;
    const comments = await github.paginate(github.rest.issues.listComments, {
        owner,
        repo,
        issue_number,
        per_page: 100,
    });

    const existing = comments.find(
        (comment) =>
            typeof comment.body === "string" &&
            comment.body.includes(MARKER) &&
            comment.user?.type === "Bot"
    );

    if (existing) {
        await github.rest.issues.updateComment({
            owner,
            repo,
            comment_id: existing.id,
            body,
        });
        return;
    }

    await github.rest.issues.createComment({
        owner,
        repo,
        issue_number,
        body,
    });
}

module.exports = async function lintFeedback({ github, context, core }) {
    try {
        const findings = collectFindings(process.env);

        if (findings.length === 0) {
            core.info("No actionable lint feedback to post.");
            return;
        }

        await upsertComment({
            github,
            context,
            body: buildBody(findings),
        });
    } catch (error) {
        const message = error instanceof Error ? error.stack || error.message : String(error);
        core.warning(`Lint feedback helper skipped: ${message}`);
    }
};

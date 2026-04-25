const fs = require("node:fs");
const path = require("node:path");

const SYSTEM_PROMPT_PATH = path.join(
  __dirname,
  "..",
  "crates",
  "dictate-core",
  "src",
  "postprocess",
  "prompts",
  "cleanup.txt",
);

function escapeXml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function buildUserMessage(text, context) {
  const trimmedContext = String(context || "").trim();
  if (!trimmedContext) {
    return String(text);
  }

  return [
    "<transcription>",
    escapeXml(text),
    "</transcription>",
    "",
    "<context>",
    escapeXml(trimmedContext),
    "</context>",
  ].join("\n");
}

class PostprocessContextProvider {
  constructor(options = {}) {
    this.options = options;
    this.config = options.config || {};
    this.systemPrompt = fs.readFileSync(SYSTEM_PROMPT_PATH, "utf8").trim();
  }

  id() {
    return this.options.id || "dictate-postprocess-context";
  }

  async callApi(prompt, context) {
    const vars = context && context.vars ? context.vars : {};
    const response = await fetch(this.config.baseUrl, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${process.env[this.config.apiKeyEnvar] || ""}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        model: this.config.model,
        temperature: this.config.temperature ?? 0,
        messages: [
          { role: "system", content: this.systemPrompt },
          {
            role: "user",
            content: buildUserMessage(
              vars.raw_transcript ?? prompt,
              vars.context,
            ),
          },
        ],
      }),
    });

    const bodyText = await response.text();
    if (!response.ok) {
      return {
        error: `post-process provider returned ${response.status}: ${bodyText}`,
      };
    }

    let body;
    try {
      body = JSON.parse(bodyText);
    } catch (error) {
      return { error: `invalid provider JSON: ${error.message}: ${bodyText}` };
    }

    const output = body.choices && body.choices[0]?.message?.content;
    if (typeof output !== "string") {
      return { error: `provider response contained no output: ${bodyText}` };
    }

    return { output: output.trim() };
  }
}

module.exports = PostprocessContextProvider;

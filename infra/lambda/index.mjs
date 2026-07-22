import { DynamoDBClient, PutItemCommand } from "@aws-sdk/client-dynamodb";

const client = new DynamoDBClient({});
const TABLE_NAME = process.env.TABLE_NAME;
const ALLOWED_PLATFORMS = new Set(["macos", "windows"]);

export const handler = async (event) => {
  const method = event.requestContext?.http?.method;
  if (method !== "POST") {
    return { statusCode: 405, body: "Method Not Allowed" };
  }

  let body;
  try {
    body = JSON.parse(event.body || "{}");
  } catch {
    return { statusCode: 400, body: "Invalid JSON" };
  }

  const platform = ALLOWED_PLATFORMS.has(body.platform) ? body.platform : "unknown";
  const now = new Date().toISOString();
  const sortKey = `${now}#${crypto.randomUUID()}`;
  const headers = event.headers || {};

  await client.send(new PutItemCommand({
    TableName: TABLE_NAME,
    Item: {
      platform: { S: platform },
      clicked_at: { S: sortKey },
      user_agent: { S: (headers["user-agent"] || "").slice(0, 300) },
      referrer: { S: (headers["referer"] || headers["Referer"] || "").slice(0, 300) },
    },
  }));

  return {
    statusCode: 204,
    headers: { "Access-Control-Allow-Origin": "*" },
    body: "",
  };
};

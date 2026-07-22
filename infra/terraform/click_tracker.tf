# DynamoDB table storing one item per download-button click.
resource "aws_dynamodb_table" "download_clicks" {
  name         = "${var.project_name}-download-clicks"
  billing_mode = "PAY_PER_REQUEST" # scales to zero, no fixed cost
  hash_key     = "platform"
  range_key    = "clicked_at"

  attribute {
    name = "platform"
    type = "S"
  }

  attribute {
    name = "clicked_at"
    type = "S"
  }

  tags = {
    Project   = var.project_name
    ManagedBy = "Terraform"
  }
}

data "archive_file" "click_tracker" {
  type        = "zip"
  source_file = "${path.module}/../lambda/index.mjs"
  output_path = "${path.module}/build/click-tracker.zip"
}

resource "aws_iam_role" "click_tracker" {
  name = "${var.project_name}-click-tracker-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action    = "sts:AssumeRole"
      Effect    = "Allow"
      Principal = { Service = "lambda.amazonaws.com" }
    }]
  })

  tags = {
    Project   = var.project_name
    ManagedBy = "Terraform"
  }
}

resource "aws_iam_role_policy_attachment" "basic_logs" {
  role       = aws_iam_role.click_tracker.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

# Scoped to exactly this table — no broader DynamoDB access.
resource "aws_iam_role_policy" "dynamodb_write" {
  name = "${var.project_name}-click-tracker-dynamodb-write"
  role = aws_iam_role.click_tracker.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["dynamodb:PutItem"]
      Resource = aws_dynamodb_table.download_clicks.arn
    }]
  })
}

resource "aws_lambda_function" "click_tracker" {
  function_name    = "${var.project_name}-click-tracker"
  role             = aws_iam_role.click_tracker.arn
  handler          = "index.handler"
  runtime          = "nodejs20.x"
  filename         = data.archive_file.click_tracker.output_path
  source_code_hash = data.archive_file.click_tracker.output_base64sha256
  timeout          = 5
  memory_size      = 128

  environment {
    variables = {
      TABLE_NAME = aws_dynamodb_table.download_clicks.name
    }
  }

  tags = {
    Project   = var.project_name
    ManagedBy = "Terraform"
  }
}

# Public HTTPS endpoint via API Gateway HTTP API (Lambda Function URL kept
# returning an unexplained 403 despite a correct public resource policy —
# API Gateway is the more battle-tested path for anonymous public access).
resource "aws_apigatewayv2_api" "click_tracker" {
  name          = "${var.project_name}-click-tracker"
  protocol_type = "HTTP"

  cors_configuration {
    allow_origins = var.allowed_origins
    allow_methods = ["POST"]
    allow_headers = ["content-type"]
    max_age       = 300
  }
}

resource "aws_apigatewayv2_integration" "click_tracker" {
  api_id                 = aws_apigatewayv2_api.click_tracker.id
  integration_type       = "AWS_PROXY"
  integration_uri        = aws_lambda_function.click_tracker.invoke_arn
  payload_format_version = "2.0"
}

resource "aws_apigatewayv2_route" "click_tracker" {
  api_id    = aws_apigatewayv2_api.click_tracker.id
  route_key = "POST /track"
  target    = "integrations/${aws_apigatewayv2_integration.click_tracker.id}"
}

resource "aws_apigatewayv2_stage" "default" {
  api_id      = aws_apigatewayv2_api.click_tracker.id
  name        = "$default"
  auto_deploy = true
}

resource "aws_lambda_permission" "apigw" {
  statement_id  = "AllowAPIGatewayInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.click_tracker.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.click_tracker.execution_arn}/*/*"
}

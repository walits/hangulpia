output "click_tracker_url" {
  description = "API endpoint the homepage POSTs click events to"
  value       = "${aws_apigatewayv2_stage.default.invoke_url}track"
}

output "dynamodb_table_name" {
  description = "DynamoDB table storing click events"
  value       = aws_dynamodb_table.download_clicks.name
}

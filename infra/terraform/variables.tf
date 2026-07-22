variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "ap-northeast-2"
}

variable "aws_profile" {
  description = "AWS CLI profile"
  type        = string
  default     = "default"
}

variable "project_name" {
  description = "Project name"
  type        = string
  default     = "hangulpia"
}

variable "allowed_origins" {
  description = "Origins allowed to call the click tracker Lambda URL"
  type        = list(string)
  default     = ["https://hangulpia.com", "https://www.hangulpia.com"]
}

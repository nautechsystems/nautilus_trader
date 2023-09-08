```
export CODEARTIFACT_AUTH_TOKEN=`aws codeartifact get-authorization-token --domain a23r --domain-owner 622371988550 --region ap-northeast-1 --query authorizationToken --output text`
```
```
export POETRY_HTTP_BASIC_ARTIFACT_USERNAME=aws
export POETRY_HTTP_BASIC_ARTIFACT_PASSWORD=CODEARTIFACT_AUTH_TOKEN
```
```
poetry publish --repository artifact
```
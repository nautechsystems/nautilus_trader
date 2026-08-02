ARG BASE_IMAGE=scratch
FROM ${BASE_IMAGE} AS source
FROM scratch
COPY --from=source / /

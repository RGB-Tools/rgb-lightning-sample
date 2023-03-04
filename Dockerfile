ARG BUILDER_DIR=/srv/rgb

FROM rust:1.67-slim-bullseye as builder

RUN apt-get update \
    && apt-get -y install --no-install-recommends \
        build-essential cmake git pkg-config \
        libpq-dev libssl-dev libzmq3-dev libsqlite3-dev

ARG SRC_DIR=/usr/local/src/rgb
WORKDIR "$SRC_DIR"

ARG BUILDER_DIR
ARG VER_STORE="0.9.0"
ARG VER_NODE="0.9.1"
ARG VER_CLI="0.9.1"
ARG VER_STD="0.9.0"
ARG VER_RGB20="0.9.0"
RUN cargo install store_daemon --version "${VER_STORE}" \
        --debug --locked --all-features --root "${BUILDER_DIR}"
RUN cargo install rgb_node --version "${VER_NODE}" \
        --debug --locked --all-features --root "${BUILDER_DIR}"
COPY ./client_side_validation /opt/client_side_validation
COPY ./bp-core /opt/bp-core
COPY ./rgb-core /opt/rgb-core
COPY ./rgb-node /opt/rgb-node
RUN cargo install rgb_node \
        --path "/opt/rgb-node/" \
        --debug --locked --all-features --root "${BUILDER_DIR}"

FROM debian:bullseye-slim

ARG DATA_DIR=/var/lib/rgb
ARG USER=rgb
ENV USER=${USER}

RUN apt-get update \
    && apt-get -y install --no-install-recommends \
       libsqlite3-0 libssl1.1 supervisor \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

RUN adduser --home "${DATA_DIR}" --shell /bin/bash --disabled-login \
        --gecos "${USER} user" ${USER}

ARG BUILDER_DIR
ARG BIN_DIR=/usr/local/bin
COPY --from=builder --chown=${USER}:${USER} \
     "${BUILDER_DIR}/bin/" "${BIN_DIR}"

COPY supervisor.conf /srv/supervisor.conf
COPY entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/entrypoint.sh

WORKDIR "${BIN_DIR}"

VOLUME "$DATA_DIR"

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]

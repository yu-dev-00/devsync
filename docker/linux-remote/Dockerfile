# A throwaway Linux remote for checking devsync against a non-Windows execution copy.
#
#   docker build -f docker/linux-remote/Dockerfile -t devsync-linux-remote .
#   docker run -d --init --name devsync-linux -p 2222:22 -e AUTHORIZED_KEY="$(cat key.pub)" devsync-linux-remote
#   ssh -i key -p 2222 dev@localhost devsync --version
#
# --init matters: without it sshd is PID 1 and orphan reaping differs from a real host.
# Keep remote_dir inside the container; a bind-mounted Windows folder brings back
# NTFS case-insensitivity and permissions, which is exactly what this is meant to avoid.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-server procps \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd \
 && ssh-keygen -A \
 && useradd -m -s /bin/bash dev \
 && usermod -p '*' dev
COPY --from=build /src/target/release/devsync /usr/local/bin/devsync
EXPOSE 22
# The key arrives at run time so no credential is baked into the image.
CMD install -d -m 700 -o dev -g dev /home/dev/.ssh \
 && printf '%s\n' "$AUTHORIZED_KEY" > /home/dev/.ssh/authorized_keys \
 && chown dev:dev /home/dev/.ssh/authorized_keys \
 && chmod 600 /home/dev/.ssh/authorized_keys \
 && exec /usr/sbin/sshd -D -e

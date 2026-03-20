FROM ghcr.io/crazy-max/samba:4.21.4

RUN apk add --no-cache \
    bash \
    inotify-tools \
    samba \
    tini \
    vmtouch --repository=http://dl-cdn.alpinelinux.org/alpine/edge/testing \
    && rm -rf /tmp/*

COPY rootfs/ /
RUN chmod +x /etc/services.d/cache-manager/run

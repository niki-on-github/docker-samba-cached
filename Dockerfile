FROM ghcr.io/crazy-max/samba:latest

RUN apk --update --no-cache add \
    bash \
    inotify-tools \
    vmtouch \
    tini \
    && rm -rf /tmp/*

COPY rootfs/ /
RUN chmod +x /etc/services.d/cache-manager/run

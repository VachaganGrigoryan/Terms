# Image used to record the README GIFs (see record.sh). VHS is pinned by digest.
FROM ghcr.io/charmbracelet/vhs@sha256:ea49a6a1c529be83153e88321892b5585964418f0b9055e8c1e0d732194234a3
RUN apt-get update \
    && apt-get install -y --no-install-recommends zsh git less \
    && rm -rf /var/lib/apt/lists/*
COPY .build/terms /usr/local/bin/terms
COPY setup-demo.sh /tmp/setup-demo.sh
RUN sh /tmp/setup-demo.sh && rm /tmp/setup-demo.sh
COPY zshrc /root/.zdot/.zshrc
ENV ZDOTDIR=/root/.zdot TERMS_BIN=/usr/local/bin/terms
WORKDIR /demo

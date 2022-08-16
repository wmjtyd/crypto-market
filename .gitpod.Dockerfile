FROM gitpod/workspace-c:2022-08-04-13-40-17 AS ta-lib-stage
ENV FILENAME ta-lib-0.4.0-src.tar.gz
USER gitpod

WORKDIR /tmp
RUN sudo apt install build-essential wget -y \
        && wget "https://artiya4u.keybase.pub/TA-lib/${FILENAME}" \
        && tar -xvf "$FILENAME"

WORKDIR /tmp/ta-lib
RUN ./configure --prefix=/tmp/talib \
        && make \
        && make install

FROM gitpod/workspace-c:2022-08-04-13-40-17 AS nanomsg-stage
ENV VERSION 1.2
USER gitpod

WORKDIR /tmp
RUN sudo apt install build-essential wget cmake -y \
        && wget -O "nanomsg.zip" "https://github.com/nanomsg/nanomsg/archive/refs/tags/${VERSION}.zip" \
        && unzip "nanomsg.zip"

WORKDIR /tmp/nanomsg-${VERSION}/build
RUN cmake -DCMAKE_INSTALL_PREFIX:PATH=/tmp/nanomsg .. \
        && cmake --build . \
        && ctest . -j16 \
        && cmake --build . --target install

FROM gitpod/workspace-rust:2022-08-04-13-40-17 AS cargo-install-stage
USER gitpod

RUN cargo install --root=/tmp/cargo-bin cargo-udeps

FROM gitpod/workspace-full:2022-08-04-13-40-17
USER gitpod

COPY --from=nanomsg-stage /tmp/nanomsg/ /usr/
COPY --from=ta-lib-stage /tmp/talib/ /usr/

RUN sudo ldconfig && pip install ta-lib --no-cache-dir

COPY --from=cargo-install-stage /tmp/cargo-bin /home/gitpod/.cargo/

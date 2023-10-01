FROM rockylinux/rockylinux:8.8

USER root

# Install basic packages
RUN yum install -y wget bzip2

# Install packages for compiling PostgreSQL
RUN yum install -y gcc readline readline-devel flex perl zlib-devel

RUN useradd docker

USER docker
WORKDIR /home/docker

RUN wget https://ftp.postgresql.org/pub/source/v15.4/postgresql-15.4.tar.bz2
RUN tar xvjf postgresql-15.4.tar.bz2

RUN wget https://github.com/ossc-db/pg_statsinfo/archive/refs/tags/15.1.tar.gz
RUN tar xvzf 15.1.tar.gz

RUN mv pg_statsinfo-15.1 postgresql-15.4/contrib/

WORKDIR /home/docker/postgresql-15.4
RUN ./configure --prefix=/home/docker/.postgresql-15.4
RUN make && make install

ENV PATH $PATH:/home/docker/.postgresql-15.4/bin
RUN mkdir /home/docker/data
RUN pg_ctl initdb -D /home/docker/data
RUN echo "listen_addresses = '*'"  >> /home/docker/data/postgresql.conf
RUN echo "host    all             all             0.0.0.0/0               trust" >> /home/docker/data/pg_hba.conf

WORKDIR /home/docker/postgresql-15.4/contrib/pg_statsinfo-15.1
RUN make && make install
RUN echo "shared_preload_libraries = 'pg_statsinfo'" >> /home/docker/data/postgresql.conf
RUN echo "log_filename = 'pg_statsinfo-%Y-%m-%d_%H%M%S.log'" >> /home/docker/data/postgresql.conf
RUN echo "pg_statsinfo.snapshot_interval = 2147483647" >> /home/docker/data/postgresql.conf

WORKDIR /home/docker
RUN echo "export PATH=/home/docker/.postgresql-15.4/bin:$PATH" >> /home/docker/.bashrc
RUN echo "export PGDATA=/home/docker/data" >> /home/docker/.bashrc

# Use `postgres` instead of `pg_ctl` cuz a postgres process needs to run in a front-end job
ENTRYPOINT postgres -D /home/docker/data

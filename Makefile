rpi?=true
target?=aarch64-unknown-linux-gnu
ifeq ($(rpi), true)
target=armv7-unknown-linux-gnueabihf
endif

BIN_DIR=$(PWD)/target/$(target)/release
REMOTE_URL=192.168.1.69
REMOTE_DIR=/storage/lirc-changer-rust
BINARY_NAME=changer
SERVICE_NAME=lirc-changer-rust.service
###############################################################################
build: $(BIN_DIR)/$(BINARY_NAME)

$(BIN_DIR)/$(BINARY_NAME): src/main.rs src/listener/main.rs
	cross build --release --target=$(target)
###############################################################################
build/local: 
	cargo build --release
###############################################################################
ssh:
	ssh root@$(REMOTE_URL)
###############################################################################
clean:
	rm -f $(BIN_DIR)/$(BINARY_NAME)
clean/all:
	rm -r -f target
###############################################################################
copy: $(BIN_DIR)/$(BINARY_NAME)
	ssh root@$(REMOTE_URL) "mkdir -p $(REMOTE_DIR)"
	scp $(BIN_DIR)/$(BINARY_NAME) root@$(REMOTE_URL):$(REMOTE_DIR)
	scp $(BIN_DIR)/listener root@$(REMOTE_URL):$(REMOTE_DIR)

run-remote:
	ssh -t root@$(REMOTE_URL) "cd $(REMOTE_DIR) && ./$(BINARY_NAME)"

test/unit:
	cargo test

test/lint:
	cargo clippy

copy/service:
	ssh root@$(REMOTE_URL) "mkdir -p $(REMOTE_DIR)/logs"
	scp config/$(SERVICE_NAME) root@$(REMOTE_URL):/storage/.config/system.d
	ssh root@$(REMOTE_URL) "systemctl enable $(SERVICE_NAME)"

restart:
	ssh root@$(REMOTE_URL) "systemctl restart $(SERVICE_NAME)"

stop:
	ssh root@$(REMOTE_URL) "systemctl stop $(SERVICE_NAME)"

logs:
	ssh root@$(REMOTE_URL) "cat $(REMOTE_DIR)/logs/service.err"
	ssh root@$(REMOTE_URL) "cat $(REMOTE_DIR)/logs/service.log"    

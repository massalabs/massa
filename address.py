#!/usr/bin/env python3.8

import numpy as np
import hashlib
import base58
import random
from blake3 import blake3
import varint
import ed25519
import json
import base64
import binascii
import varint
import hashlib
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.fernet import Fernet

WALLET_PWD = "test"

    
def encrypt(data: str, pwd: str):
    salt = binascii.unhexlify('646464646464646464646464')
    salt_encoded = base64.b64decode(salt)
    key = hashlib.pbkdf2_hmac('sha256', pwd.encode('utf-8'), salt_encoded, 10000, 32)
    nonce = binascii.unhexlify('656565656565656565656565')
    cipher = AESGCM(key)
    encrypted = cipher.encrypt(nonce, data.encode("utf-8"), None)
    result = bytearray()
    result.extend(varint.encode(0))
    result.extend(salt)
    result.extend(nonce)
    result.extend(encrypted)
    return result

print(base64.b64decode("qetr"))

class KeyPair:
    def __init__(self, secret_key=None, public_key=None):
            self.secret_key = secret_key
            self.public_key = public_key
    
    def random():
        signing_key, verifying_key = ed25519.create_keypair()
        return KeyPair(secret_key=signing_key, public_key=verifying_key)

    def from_secret_massa_encoded(private: str):
        # Strip identifier
        private = private[1:]
        # Decode base58
        private = base58.b58decode_check(private)
        # Decode varint
        version = varint.decode_bytes(private)
        # Get rest (for the moment versions are little)
        secret_key = private[1:]
        # decode privkey
        secret_key = ed25519.keys.SigningKey(secret_key)
        public_key = secret_key.get_verifying_key()
        return KeyPair(secret_key=secret_key, public_key=public_key)

    def get_public_massa_encoded(self):
        return 'P' + base58.b58encode_check(varint.encode(0) + self.public_key.to_bytes()).decode("utf-8")

    def get_secret_massa_encoded(self):
        return 'S' + base58.b58encode_check(varint.encode(0) + self.secret_key.to_seed()).decode("utf-8")

def decode_pubkey_to_bytes(pubkey):
    return base58.b58decode_check(pubkey[1:])[1:]

def deduce_address(pubkey):
    return 'AU' + base58.b58encode_check(varint.encode(0) + blake3(pubkey.to_bytes()).digest()).decode("utf-8")

def get_address_thread(address):
    address_bytes = base58.b58decode_check(address[2:])[1:]
    return np.frombuffer(address_bytes, dtype=np.uint8)[0] / 8


# can use a loop here to make multiple config files
keypair = KeyPair.random()
pub_key=(keypair.get_public_massa_encoded())
scrt_key=(keypair.get_secret_massa_encoded())
addr=(deduce_address(keypair.public_key))

# node_keypair = {"secret_key": srv["node_privkey"], "public_key": srv["node_pubkey"]}
# staking_setup = {srv["staking_address"]: {"secret_key": srv["staking_privkey"], "public_key": srv["staking_pubkey"]}}



# node_keypair 
# Data to be stored in the JSON file
data = {
    "secret_key": scrt_key,
    "public_key": pub_key
}

# Path to the JSON file
file_path = "./massa-node/config/node_keypair.key"

# Write data to the JSON file
with open(file_path, "w") as file:
    json.dump(data, file)

print(f"JSON file '{file_path}' created successfully.")


# staking_setup

# staking_setup = {srv["staking_address"]: {"secret_key": srv["staking_privkey"], "public_key": srv["staking_pubkey"]}}
staking_setup = {
     addr:{ "secret_key":scrt_key,"public_key":pub_key}
 }
# Path to the JSON file
file_path_staking = "./massa-node/config/staking_wallet.dat"

with open(file_path_staking, "wb") as json_file:
    # json_file.write(cipher.encrypt(json.dumps(staking_setup), WALLET_PWD))
    json_file.write(encrypt(json.dumps(staking_setup), WALLET_PWD))


print(f"JSON file '{file_path_staking}' created successfully.")

print (WALLET_PWD)
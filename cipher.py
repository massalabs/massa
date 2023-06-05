import base64
import binascii
import varint
import hashlib
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

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
#!/usr/bin/env python3
"""Decode a Wireshark USBPcap capture (.pcapng, link type 249) and list what a device was sent.

Usage: python3 -I tools/decode_usbpcap.py captures/ngenuity.pcapng

Prints the devices seen, then every distinct HID SET_REPORT payload sent to each device, with the time
range it was in use and how often it was sent. Written to learn the DuoCast lighting protocol from our
own captures; standard library only.
"""
import collections
import struct
import sys


def read_pcapng(path):
    """Yield (seconds, link type, packet bytes) for every packet in a pcapng file."""
    data = open(path, 'rb').read()
    pos, ifaces = 0, []
    while pos + 12 <= len(data):
        block_type, length = struct.unpack_from('<II', data, pos)
        body = data[pos + 8:pos + length - 4]
        if block_type == 1:  # interface description: link type and timestamp resolution
            link = struct.unpack_from('<H', body, 0)[0]
            resolution, opts = 6, body[8:]
            while len(opts) >= 4:
                code, size = struct.unpack_from('<HH', opts, 0)
                if code == 0:
                    break
                if code == 9:  # if_tsresol
                    resolution = opts[4]
                opts = opts[4 + ((size + 3) & ~3):]
            ifaces.append((link, resolution))
        elif block_type == 6:  # enhanced packet
            iface, ts_high, ts_low, captured, _ = struct.unpack_from('<IIIII', body, 0)
            link, resolution = ifaces[iface]
            ticks = 10 ** resolution if resolution < 128 else 2 ** (resolution & 127)
            yield ((ts_high << 32) | ts_low) / ticks, link, body[20:20 + captured]
        pos += length


def parse_usbpcap(packet):
    """Split a USBPcap packet into its header fields and payload."""
    header_len, _irp, _status, function, info, bus, device, endpoint, transfer, data_len = struct.unpack_from('<HQIHBHHBBI', packet, 0)
    return dict(bus=bus, device=device, endpoint=endpoint, transfer=transfer, function=function,
                to_host=bool(info & 1), stage=packet[27] if header_len > 27 else None,
                payload=packet[header_len:header_len + data_len])


def main(path):
    packets = [(t, parse_usbpcap(p)) for t, link, p in read_pcapng(path) if link == 249]
    if not packets:
        sys.exit('no USBPcap packets found')
    start = packets[0][0]

    print('devices (from device descriptors):')
    for _, p in packets:
        d = p['payload']
        if len(d) == 18 and d[1] == 1 and p['to_host']:
            print('  bus %d device %d: vendor %04x product %04x' % (p['bus'], p['device'], *struct.unpack_from('<HH', d, 8)))

    # A control transfer begins with an 8-byte setup packet (stage 0), followed by the data in the same
    # direction. SET_REPORT is bmRequestType 0x21, bRequest 0x09; wValue = report type << 8 | report id.
    seen = collections.OrderedDict()
    for t, p in packets:
        d = p['payload']
        if p['transfer'] == 2 and p['stage'] == 0 and not p['to_host'] and len(d) > 8 and d[0] == 0x21 and d[1] == 0x09:
            key = (p['device'], struct.unpack_from('<H', d, 2)[0], d[8:])
            first, last, count = seen.get(key, (t - start, 0, 0))
            seen[key] = (first, t - start, count + 1)

    print('\ndistinct SET_REPORT payloads (trailing zeros trimmed):')
    for (device, value, body), (first, last, count) in seen.items():
        kind = {1: 'input', 2: 'output', 3: 'feature'}.get(value >> 8, '?')
        print('  dev %d %s report id %d  %6.2fs-%6.2fs  x%-4d %s' % (device, kind, value & 0xff, first, last, count, body.rstrip(b'\0').hex(' ')))


if __name__ == '__main__':
    main(sys.argv[1])

/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BondConfig } from './BondConfig';
import type { DnsConfig } from './DnsConfig';
import type { IpAssignment } from './IpAssignment';
import type { IpMode } from './IpMode';
import type { VlanConfig } from './VlanConfig';
export type NetworkInterfaceSettings = {
    /**
     * IPv4 address used when static mode is active
     */
    address?: string;
    bond?: (null | BondConfig);
    dns?: DnsConfig;
    /**
     * Default gateway in static mode
     */
    gateway?: string;
    gateways?: Array<string>;
    ipv4?: Array<IpAssignment>;
    ipv6?: Array<IpAssignment>;
    ipv6_gateway?: string;
    ipv6_mode?: IpMode;
    mac?: string | null;
    mode?: IpMode;
    name?: string;
    /**
     * Subnet mask associated with `address`
     */
    netmask?: string;
    vlan?: (null | VlanConfig);
};


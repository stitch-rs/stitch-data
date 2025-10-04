using System;

namespace CBP.Utilities;

/// <summary>
/// Provides ZigZag encoding/decoding for efficient negative integer serialization
/// </summary>
public static class ZigZagEncoding
{
    /// <summary>
    /// Encodes a signed integer using ZigZag encoding
    /// </summary>
    /// <param name="value">Signed integer to encode</param>
    /// <returns>Unsigned integer with ZigZag encoding applied</returns>
    public static int Encode(int value) => (value << 1) ^ (value >> 31);
    
    /// <summary>
    /// Decodes a ZigZag encoded integer back to signed form
    /// </summary>
    /// <param name="encoded">ZigZag encoded value</param>
    /// <returns>Original signed integer</returns>
    public static int Decode(int encoded) => (encoded >> 1) ^ (-(encoded & 1));
    
    /// <summary>
    /// Encodes a signed long using ZigZag encoding
    /// </summary>
    public static long Encode(long value) => (value << 1) ^ (value >> 63);
    
    /// <summary>
    /// Decodes a ZigZag encoded long back to signed form
    /// </summary>
    public static long Decode(long encoded) => (encoded >> 1) ^ (-(encoded & 1));
}

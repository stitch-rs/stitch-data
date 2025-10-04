namespace CBP;

/// <summary>
/// Represents a type that can be serialized to and from binary format
/// </summary>
public interface IBinarySerializable
{
    /// <summary>
    /// Serializes the object to binary format using the specified writer
    /// </summary>
    /// <param name="writer">Binary writer to use for serialization</param>
    void Serialize(IBinaryWriter writer);
    
    /// <summary>
    /// Deserializes the object from binary format using the specified reader
    /// </summary>
    /// <param name="reader">Binary reader to use for deserialization</param>
    void Deserialize(IBinaryReader reader);
}

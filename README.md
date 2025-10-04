# Compact Binary Protocol (CBP)

High-performance binary serialization protocol for games, IoT, and real-time applications.

## Features

- 🚀 **Maximum Performance** - Optimized for speed and memory usage
- 📦 **Compact Size** - 60-80% smaller than JSON
- 🔧 **Cross-Platform** - C#, TypeScript, Rust, Pawn support
- 🎯 **Flexible** - Multiple serialization modes
- 🔒 **Type-Safe** - Contract-first approach

## Quick Start

```csharp
// Define your contract
[BinaryContract]
public struct PlayerState : IBinarySerializable
{
    [Field(1)] public int Id;
    [Field(2)] public string Name;
    
    public void Serialize(IBinaryWriter writer) => writer.Write(Id).Write(Name);
    public void Deserialize(IBinaryReader reader)
    {
        Id = reader.ReadCompactInt();
        Name = reader.ReadCompactString();
    }
}

// Serialize/Deserialize
var player = new PlayerState { Id = 123, Name = "John" };
var bytes = CbpSerializer.Serialize(player);
var restored = CbpSerializer.Deserialize<PlayerState>(bytes);
```
